use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use async_trait::async_trait;
use rtc::{
    media_stream::MediaStreamTrack,
    peer_connection::configuration::media_engine::MediaEngine,
    rtp_transceiver::rtp_sender::{RTCRtpCodingParameters, RTCRtpEncodingParameters},
};
use tokio::sync::{Mutex, mpsc, oneshot};
use webrtc::{
    media_stream::{
        Track,
        track_local::{TrackLocal, static_rtp::TrackLocalStaticRTP},
        track_remote::{TrackRemote, TrackRemoteEvent},
    },
    peer_connection::{
        PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler, RTCConfigurationBuilder,
        RTCIceCandidateInit, RTCIceServer, RTCPeerConnectionIceEvent, RTCSessionDescription,
    },
    rtp_transceiver::RtpSender,
};

use crate::signaling::{CandidateMessage, SignalMessage};

#[derive(Default)]
pub struct WebRtcAgent {
    connections: Arc<Mutex<HashMap<String, Arc<dyn PeerConnection>>>>,
    senders: Arc<Mutex<HashMap<String, mpsc::UnboundedSender<String>>>>,
    relays: Arc<Mutex<HashMap<String, RelayTrack>>>,
    relay_tasks: Arc<Mutex<HashMap<String, oneshot::Sender<()>>>>,
    published_tracks: Arc<Mutex<HashMap<String, PublishedTrack>>>,
    ready_peers: Arc<Mutex<HashSet<String>>>,
    negotiation_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    local_offers: Arc<Mutex<HashSet<String>>>,
    queued_negotiations: Arc<Mutex<HashSet<String>>>,
    relay_lifecycle_lock: Arc<Mutex<()>>,
    udp_addrs: Arc<Vec<String>>,
}

struct PublishedTrack {
    source_peer_key: String,
    track: Arc<dyn TrackRemote>,
}

struct RelayTrack {
    destination_peer_key: String,
    sender: Arc<dyn RtpSender>,
    output: Arc<TrackLocalStaticRTP>,
}

impl std::fmt::Debug for WebRtcAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebRtcAgent").finish_non_exhaustive()
    }
}

impl Clone for WebRtcAgent {
    fn clone(&self) -> Self {
        Self {
            connections: self.connections.clone(),
            senders: self.senders.clone(),
            relays: self.relays.clone(),
            relay_tasks: self.relay_tasks.clone(),
            published_tracks: self.published_tracks.clone(),
            ready_peers: self.ready_peers.clone(),
            negotiation_locks: self.negotiation_locks.clone(),
            local_offers: self.local_offers.clone(),
            queued_negotiations: self.queued_negotiations.clone(),
            relay_lifecycle_lock: self.relay_lifecycle_lock.clone(),
            udp_addrs: self.udp_addrs.clone(),
        }
    }
}

#[derive(Clone)]
struct PeerConnectionHandler {
    peer_key: String,
    agent: WebRtcAgent,
}

#[async_trait]
impl PeerConnectionEventHandler for PeerConnectionHandler {
    async fn on_ice_candidate(&self, event: RTCPeerConnectionIceEvent) {
        let candidate = event.candidate;
        let Ok(candidate) = candidate.to_json() else {
            return;
        };
        let Some((room, peer_id)) = self.peer_key.split_once(':') else {
            return;
        };
        self.agent
            .send_to_peer(
                &self.peer_key,
                SignalMessage {
                    kind: "candidate".to_owned(),
                    room: room.to_owned(),
                    peer_id: peer_id.to_owned(),
                    from: "server".to_owned(),
                    to: peer_id.to_owned(),
                    sdp: String::new(),
                    candidate: Some(CandidateMessage {
                        candidate: candidate.candidate,
                        sdp_mid: candidate.sdp_mid,
                        sdp_mline_index: candidate.sdp_mline_index,
                    }),
                },
            )
            .await;
    }

    async fn on_track(&self, track: Arc<dyn TrackRemote>) {
        let agent = self.agent.clone();
        let source_key = self.peer_key.clone();
        tokio::spawn(async move {
            if let Err(error) = agent.relay_track(source_key.clone(), track).await {
                eprintln!("RTP relay error: {error}");
                agent
                    .notify_room_relay_error(&source_key, &error.to_string())
                    .await;
            }
        });
    }
}

impl WebRtcAgent {
    pub fn new() -> Self {
        Self {
            udp_addrs: Arc::new(vec!["0.0.0.0:0".to_owned()]),
            ..Self::default()
        }
    }

    #[cfg(test)]
    fn with_udp_addrs(udp_addrs: Vec<String>) -> Self {
        Self {
            udp_addrs: Arc::new(udp_addrs),
            ..Self::new()
        }
    }

    pub async fn register_peer(
        &self,
        room: &str,
        peer_id: &str,
        tx: mpsc::UnboundedSender<String>,
    ) {
        self.senders
            .lock()
            .await
            .insert(self.key(room, peer_id), tx);
    }

    pub async fn process_signal(
        &self,
        signal: &SignalMessage,
        room: &str,
        peer_id: &str,
    ) -> Result<Option<SignalMessage>, webrtc::error::Error> {
        let key = self.key(room, peer_id);
        let negotiation_lock = self.negotiation_lock(&key).await;
        let _negotiation_guard = negotiation_lock.lock().await;
        match signal.kind.as_str() {
            "offer" => {
                let pc = self.get_or_create(&key).await?;
                if self.local_offers.lock().await.remove(&key) {
                    pc.set_local_description(RTCSessionDescription::rollback(None)?)
                        .await?;
                }
                pc.set_remote_description(RTCSessionDescription::offer(signal.sdp.clone())?)
                    .await?;
                self.add_existing_tracks(&key, room).await?;
                let answer = pc.create_answer(None).await?;
                pc.set_local_description(answer.clone()).await?;
                self.ready_peers.lock().await.insert(key);
                self.queued_negotiations
                    .lock()
                    .await
                    .remove(&self.key(room, peer_id));
                Ok(Some(SignalMessage {
                    kind: "answer".to_owned(),
                    room: room.to_owned(),
                    peer_id: peer_id.to_owned(),
                    from: "server".to_owned(),
                    to: peer_id.to_owned(),
                    sdp: answer.sdp,
                    candidate: None,
                }))
            }
            "answer" => {
                let pc = self.get(&key).await?;
                pc.set_remote_description(RTCSessionDescription::answer(signal.sdp.clone())?)
                    .await?;
                let had_local_offer = self.local_offers.lock().await.remove(&key);
                if had_local_offer && self.queued_negotiations.lock().await.remove(&key) {
                    self.send_offer(&key, room, peer_id, &pc).await?;
                }
                Ok(None)
            }
            "candidate" => {
                if let Some(candidate) = &signal.candidate {
                    self.get(&key)
                        .await?
                        .add_ice_candidate(RTCIceCandidateInit {
                            candidate: candidate.candidate.clone(),
                            sdp_mid: candidate.sdp_mid.clone(),
                            sdp_mline_index: candidate.sdp_mline_index,
                            username_fragment: None,
                            url: None,
                        })
                        .await?;
                }
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    async fn relay_track(
        &self,
        source_key: String,
        source: Arc<dyn TrackRemote>,
    ) -> Result<(), webrtc::error::Error> {
        let lifecycle_guard = self.relay_lifecycle_lock.lock().await;
        if !self.connections.lock().await.contains_key(&source_key) {
            return Ok(());
        }
        let Some((room, _source_peer)) = source_key.split_once(':') else {
            return Ok(());
        };
        let track_id = source.track_id().await;
        let publication_key = format!("{source_key}:{track_id}");
        self.published_tracks.lock().await.insert(
            publication_key.clone(),
            PublishedTrack {
                source_peer_key: source_key.clone(),
                track: source.clone(),
            },
        );
        let destination_keys: Vec<String> = self
            .connections
            .lock()
            .await
            .keys()
            .filter(|key| is_relay_destination(key, &source_key, room))
            .cloned()
            .collect();
        let destinations: Vec<(String, Arc<dyn PeerConnection>)> = {
            let connections = self.connections.lock().await;
            destination_keys
                .into_iter()
                .filter_map(|key| connections.get(&key).cloned().map(|pc| (key, pc)))
                .collect()
        };

        for (destination_key, pc) in destinations {
            let negotiation_lock = self.negotiation_lock(&destination_key).await;
            let _negotiation_guard = negotiation_lock.lock().await;
            if !self.ready_peers.lock().await.contains(&destination_key) {
                continue;
            }
            self.add_relay_track(&publication_key, &source_key, &destination_key, &source)
                .await?;

            let destination_peer = destination_key.split(':').nth(1).unwrap_or_default();
            self.request_renegotiation_locked(&destination_key, room, destination_peer, &pc)
                .await?;
        }

        let agent = self.clone();
        let (cancel_tx, mut cancel_rx) = oneshot::channel();
        self.relay_tasks
            .lock()
            .await
            .insert(publication_key.clone(), cancel_tx);
        tokio::spawn(async move {
            let mut failed_destinations = HashSet::new();
            loop {
                tokio::select! {
                    _ = &mut cancel_rx => break,
                    event = source.poll() => {
                        let packet = match event {
                            Some(TrackRemoteEvent::OnRtpPacket(packet)) => packet,
                            Some(
                                TrackRemoteEvent::OnOpen(_)
                                | TrackRemoteEvent::OnMute
                                | TrackRemoteEvent::OnUnmute
                                | TrackRemoteEvent::OnRtcpPacket(_),
                            ) => continue,
                            Some(
                                TrackRemoteEvent::OnError
                                | TrackRemoteEvent::OnEnding
                                | TrackRemoteEvent::OnEnded,
                            ) | None => break,
                        };
                        let outputs: Vec<_> = agent
                            .relays
                            .lock()
                            .await
                            .iter()
                            .filter(|(relay_key, _)| {
                                relay_key.starts_with(&format!("{publication_key}->"))
                            })
                            .filter(|(_, relay)| {
                                !failed_destinations.contains(&relay.destination_peer_key)
                            })
                            .map(|(_, relay)| {
                                (relay.destination_peer_key.clone(), relay.output.clone())
                            })
                            .collect();
                        for (destination_key, output) in outputs {
                            let mut packet = packet.clone();
                            if let Some(ssrc) = output.ssrcs().await.first().copied() {
                                packet.header.ssrc = ssrc;
                            }
                            if let Err(error) = output.write_rtp(packet).await {
                                if failed_destinations.insert(destination_key.clone()) {
                                    agent
                                        .notify_peer_relay_error(
                                            &destination_key,
                                            &error.to_string(),
                                        )
                                        .await;
                                }
                            }
                        }
                    }
                }
            }
            agent.relay_tasks.lock().await.remove(&publication_key);
            agent.remove_publication(&publication_key).await;
        });
        drop(lifecycle_guard);
        Ok(())
    }

    async fn add_existing_tracks(
        &self,
        destination_key: &str,
        room: &str,
    ) -> Result<(), webrtc::error::Error> {
        let publications: Vec<_> = self
            .published_tracks
            .lock()
            .await
            .iter()
            .filter(|(_, publication)| {
                publication.source_peer_key != destination_key
                    && publication.source_peer_key.starts_with(&format!("{room}:"))
            })
            .map(|(key, publication)| {
                (
                    key.clone(),
                    publication.source_peer_key.clone(),
                    publication.track.clone(),
                )
            })
            .collect();

        for (publication_key, source_peer_key, source) in publications {
            self.add_relay_track(&publication_key, &source_peer_key, destination_key, &source)
                .await?;
        }
        Ok(())
    }

    async fn add_relay_track(
        &self,
        publication_key: &str,
        source_peer_key: &str,
        destination_key: &str,
        source: &Arc<dyn TrackRemote>,
    ) -> Result<(), webrtc::error::Error> {
        let relay_key = format!("{publication_key}->{destination_key}");
        if self.relays.lock().await.contains_key(&relay_key) {
            return Ok(());
        }

        let kind = source.kind().await;
        let mut codec = None;
        for ssrc in source.ssrcs().await {
            if let Some(value) = source.codec(ssrc).await {
                codec = Some(value);
                break;
            }
        }
        let Some(codec) = codec else { return Ok(()) };
        let destination = self.get(destination_key).await?;
        let remote_sdp = destination
            .remote_description()
            .await
            .map(|description| description.sdp)
            .unwrap_or_default();
        if !remote_sdp_supports_codec(
            &remote_sdp,
            &codec.mime_type,
            codec.clock_rate,
            codec.channels,
        ) {
            return Err(webrtc::error::Error::from_std(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                format!(
                    "destination {destination_key} does not support {} at {} Hz",
                    codec.mime_type, codec.clock_rate
                ),
            )));
        }
        let Some(source_ssrc) = source.ssrcs().await.first().copied() else {
            return Ok(());
        };
        let Some((room, source_peer_id)) = source_peer_key.split_once(':') else {
            return Ok(());
        };
        let output = Arc::new(TrackLocalStaticRTP::new(MediaStreamTrack::new(
            format!("relay-{room}-{source_peer_id}"),
            format!("relay-{}", source.track_id().await),
            "server-relay".to_owned(),
            kind,
            vec![RTCRtpEncodingParameters {
                rtp_coding_parameters: RTCRtpCodingParameters {
                    ssrc: Some(source_ssrc),
                    ..Default::default()
                },
                active: true,
                codec,
                ..Default::default()
            }],
        )));
        let sender = destination
            .add_track(output.clone() as Arc<dyn TrackLocal>)
            .await?;
        self.relays.lock().await.insert(
            relay_key,
            RelayTrack {
                destination_peer_key: destination_key.to_owned(),
                sender,
                output,
            },
        );
        Ok(())
    }

    async fn send_to_peer(&self, key: &str, signal: SignalMessage) {
        if let Some(tx) = self.senders.lock().await.get(key) {
            if let Ok(payload) = serde_json::to_string(&signal) {
                let _ = tx.send(payload);
            }
        }
    }

    async fn notify_room_relay_error(&self, source_key: &str, error: &str) {
        let Some((room, _)) = source_key.split_once(':') else {
            return;
        };
        let peer_keys: Vec<String> = self
            .senders
            .lock()
            .await
            .keys()
            .filter(|key| key.starts_with(&format!("{room}:")))
            .cloned()
            .collect();
        for key in peer_keys {
            self.notify_peer_relay_error(&key, error).await;
        }
    }

    async fn notify_peer_relay_error(&self, key: &str, error: &str) {
        let Some((room, peer_id)) = key.split_once(':') else {
            return;
        };
        self.send_to_peer(
            key,
            SignalMessage {
                kind: "relay-error".to_owned(),
                room: room.to_owned(),
                peer_id: peer_id.to_owned(),
                from: "server".to_owned(),
                to: peer_id.to_owned(),
                sdp: error.to_owned(),
                candidate: None,
            },
        )
        .await;
    }

    async fn send_offer(
        &self,
        key: &str,
        room: &str,
        peer_id: &str,
        pc: &Arc<dyn PeerConnection>,
    ) -> Result<(), webrtc::error::Error> {
        let offer = pc.create_offer(None).await?;
        pc.set_local_description(offer.clone()).await?;
        self.local_offers.lock().await.insert(key.to_owned());
        self.send_to_peer(
            key,
            SignalMessage {
                kind: "offer".to_owned(),
                room: room.to_owned(),
                peer_id: peer_id.to_owned(),
                from: "server".to_owned(),
                to: peer_id.to_owned(),
                sdp: offer.sdp,
                candidate: None,
            },
        )
        .await;
        Ok(())
    }

    async fn request_renegotiation_locked(
        &self,
        key: &str,
        room: &str,
        peer_id: &str,
        pc: &Arc<dyn PeerConnection>,
    ) -> Result<(), webrtc::error::Error> {
        if self.local_offers.lock().await.contains(key) {
            self.queued_negotiations.lock().await.insert(key.to_owned());
            return Ok(());
        }
        self.send_offer(key, room, peer_id, pc).await
    }

    async fn get_or_create(
        &self,
        key: &str,
    ) -> Result<Arc<dyn PeerConnection>, webrtc::error::Error> {
        if let Some(pc) = self.connections.lock().await.get(key).cloned() {
            return Ok(pc);
        }
        let config = RTCConfigurationBuilder::default()
            .with_ice_servers(vec![RTCIceServer {
                urls: vec!["stun:stun.l.google.com:19302".to_owned()],
                ..Default::default()
            }])
            .build();
        let mut media_engine = MediaEngine::default();
        media_engine.register_default_codecs()?;
        let pc = PeerConnectionBuilder::new()
            .with_configuration(config)
            .with_media_engine(media_engine)
            .with_handler(Arc::new(PeerConnectionHandler {
                peer_key: key.to_owned(),
                agent: self.clone(),
            }))
            .with_udp_addrs(self.udp_addrs.as_ref().clone())
            .build()
            .await?;
        let pc: Arc<dyn PeerConnection> = Arc::new(pc);
        self.connections
            .lock()
            .await
            .insert(key.to_owned(), pc.clone());
        Ok(pc)
    }

    async fn negotiation_lock(&self, key: &str) -> Arc<Mutex<()>> {
        let mut locks = self.negotiation_locks.lock().await;
        locks
            .entry(key.to_owned())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    async fn get(&self, key: &str) -> Result<Arc<dyn PeerConnection>, webrtc::error::Error> {
        self.connections
            .lock()
            .await
            .get(key)
            .cloned()
            .ok_or_else(|| {
                webrtc::error::Error::from_std(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("peer connection not found for {key}"),
                ))
            })
    }

    fn key(&self, room: &str, peer: &str) -> String {
        format!("{room}:{peer}")
    }

    async fn remove_publication(&self, publication_key: &str) {
        let _lifecycle_guard = self.relay_lifecycle_lock.lock().await;
        self.published_tracks.lock().await.remove(publication_key);
        if let Some(cancel) = self.relay_tasks.lock().await.remove(publication_key) {
            let _ = cancel.send(());
        }

        let relay_prefix = format!("{publication_key}->");
        let relays: Vec<RelayTrack> = {
            let mut relays = self.relays.lock().await;
            let keys = relays
                .keys()
                .filter(|relay_key| relay_key.starts_with(&relay_prefix))
                .cloned()
                .collect::<Vec<_>>();
            keys.into_iter()
                .filter_map(|relay_key| relays.remove(&relay_key))
                .collect()
        };

        for relay in relays {
            let destination_key = relay.destination_peer_key;
            let negotiation_lock = self.negotiation_lock(&destination_key).await;
            let _negotiation_guard = negotiation_lock.lock().await;
            if let Ok(pc) = self.get(&destination_key).await {
                if let Err(error) = pc.remove_track(&relay.sender).await {
                    eprintln!("failed to remove relay track: {error}");
                    continue;
                }
                let Some((destination_room, destination_peer)) = destination_key.split_once(':')
                else {
                    continue;
                };
                if let Err(error) = self
                    .request_renegotiation_locked(
                        &destination_key,
                        destination_room,
                        destination_peer,
                        &pc,
                    )
                    .await
                {
                    eprintln!("failed to renegotiate relay removal: {error}");
                }
            }
        }
    }

    pub async fn remove_peer(&self, room: &str, peer: &str) {
        let key = self.key(room, peer);
        let lifecycle_guard = self.relay_lifecycle_lock.lock().await;
        let pc = self.connections.lock().await.remove(&key);
        self.ready_peers.lock().await.remove(&key);
        let publications: Vec<String> = {
            let mut tracks = self.published_tracks.lock().await;
            let keys = tracks
                .iter()
                .filter(|(_, publication)| publication.source_peer_key == key)
                .map(|(publication_key, _)| publication_key.clone())
                .collect::<Vec<_>>();
            tracks.retain(|_, publication| publication.source_peer_key != key);
            keys
        };
        self.senders.lock().await.remove(&key);
        self.local_offers.lock().await.remove(&key);
        self.queued_negotiations.lock().await.remove(&key);
        self.relays
            .lock()
            .await
            .retain(|_, relay| relay.destination_peer_key != key);
        drop(lifecycle_guard);
        if let Some(pc) = pc {
            let _ = pc.close().await;
        }
        for publication_key in publications {
            self.remove_publication(&publication_key).await;
        }
    }
}

fn is_relay_destination(key: &str, source_key: &str, room: &str) -> bool {
    key.starts_with(&format!("{room}:")) && key != source_key
}

fn remote_sdp_supports_codec(sdp: &str, mime_type: &str, clock_rate: u32, channels: u16) -> bool {
    let expected_mime = mime_type.rsplit('/').next().unwrap_or(mime_type);
    sdp.lines()
        .filter_map(|line| line.trim().strip_prefix("a=rtpmap:"))
        .filter_map(|mapping| mapping.split_once(' ').map(|(_, codec)| codec))
        .any(|codec| {
            let mut parts = codec.split('/');
            let Some(mime) = parts.next() else {
                return false;
            };
            let Some(rate) = parts.next().and_then(|rate| rate.parse::<u32>().ok()) else {
                return false;
            };
            let channel_count = parts.next().and_then(|value| value.parse::<u16>().ok());
            mime.eq_ignore_ascii_case(expected_mime)
                && rate == clock_rate
                && (channels == 0 || channel_count.unwrap_or(1) == channels)
        })
}

#[cfg(test)]
mod tests {
    use super::{MediaEngine, WebRtcAgent, is_relay_destination, remote_sdp_supports_codec};
    use crate::signaling::{CandidateMessage, SignalMessage};
    use async_trait::async_trait;
    use rtc::{
        media_stream::MediaStreamTrack,
        rtp::packet::Packet,
        rtp_transceiver::{
            RTCRtpTransceiverDirection, RTCRtpTransceiverInit,
            rtp_sender::{
                RTCRtpCodec, RTCRtpCodingParameters, RTCRtpEncodingParameters, RtpCodecKind,
            },
        },
    };
    use std::{
        sync::{
            Arc,
            atomic::{AtomicU64, Ordering},
        },
        time::Instant,
    };
    use tokio::{
        sync::mpsc,
        task::JoinHandle,
        time::{Duration, sleep, timeout},
    };
    use webrtc::media_stream::{
        track_local::{TrackLocal, static_rtp::TrackLocalStaticRTP},
        track_remote::{TrackRemote, TrackRemoteEvent},
    };
    use webrtc::peer_connection::{
        PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler, RTCIceCandidateInit,
        RTCPeerConnectionIceEvent, RTCSessionDescription,
    };

    struct NoopHandler;

    #[async_trait]
    impl PeerConnectionEventHandler for NoopHandler {
        async fn on_ice_candidate(&self, _event: RTCPeerConnectionIceEvent) {}
    }

    struct TestPeerHandler {
        candidate_tx: mpsc::UnboundedSender<RTCIceCandidateInit>,
        track_tx: mpsc::UnboundedSender<Arc<dyn TrackRemote>>,
        connected_tx: mpsc::UnboundedSender<()>,
    }

    #[async_trait]
    impl PeerConnectionEventHandler for TestPeerHandler {
        async fn on_ice_candidate(&self, event: RTCPeerConnectionIceEvent) {
            if let Ok(candidate) = event.candidate.to_json() {
                let _ = self.candidate_tx.send(candidate);
            }
        }

        async fn on_ice_connection_state_change(
            &self,
            state: webrtc::peer_connection::RTCIceConnectionState,
        ) {
            if state == webrtc::peer_connection::RTCIceConnectionState::Connected {
                let _ = self.connected_tx.send(());
            }
        }

        async fn on_track(&self, track: Arc<dyn TrackRemote>) {
            let _ = self.track_tx.send(track);
        }
    }

    struct TestPeer {
        id: String,
        connection: Arc<dyn PeerConnection>,
        offer_rx: mpsc::UnboundedReceiver<SignalMessage>,
        packet_rx: mpsc::UnboundedReceiver<Packet>,
        connected_rx: mpsc::UnboundedReceiver<()>,
        forwarding_tasks: Vec<JoinHandle<()>>,
    }

    fn create_video_track() -> Arc<TrackLocalStaticRTP> {
        Arc::new(TrackLocalStaticRTP::new(MediaStreamTrack::new(
            "test-stream".to_owned(),
            "test-video".to_owned(),
            "loopback-video".to_owned(),
            RtpCodecKind::Video,
            vec![RTCRtpEncodingParameters {
                rtp_coding_parameters: RTCRtpCodingParameters {
                    ssrc: Some(1234),
                    ..Default::default()
                },
                active: true,
                codec: RTCRtpCodec {
                    mime_type: "video/VP8".to_owned(),
                    clock_rate: 90_000,
                    channels: 0,
                    sdp_fmtp_line: String::new(),
                    rtcp_feedback: Vec::new(),
                },
                ..Default::default()
            }],
        )))
    }

    async fn connect_test_peer(
        agent: &WebRtcAgent,
        room: &str,
        id: &str,
        local_track: Option<Arc<TrackLocalStaticRTP>>,
        receive_video: bool,
    ) -> TestPeer {
        let (candidate_tx, mut candidate_rx) = mpsc::unbounded_channel();
        let (track_tx, mut track_rx) = mpsc::unbounded_channel();
        let (connected_tx, connected_rx) = mpsc::unbounded_channel();
        let mut media_engine = MediaEngine::default();
        media_engine.register_default_codecs().unwrap();
        let connection: Arc<dyn PeerConnection> = Arc::new(
            PeerConnectionBuilder::new()
                .with_media_engine(media_engine)
                .with_handler(Arc::new(TestPeerHandler {
                    candidate_tx,
                    track_tx,
                    connected_tx,
                }))
                .with_udp_addrs(vec!["127.0.0.1:0"])
                .build()
                .await
                .unwrap(),
        );

        if let Some(track) = local_track {
            connection
                .add_track(track as Arc<dyn TrackLocal>)
                .await
                .unwrap();
        } else if receive_video {
            connection
                .add_transceiver_from_kind(
                    RtpCodecKind::Video,
                    Some(RTCRtpTransceiverInit {
                        direction: RTCRtpTransceiverDirection::Recvonly,
                        ..Default::default()
                    }),
                )
                .await
                .unwrap();
        }

        let (server_signal_tx, mut server_signal_rx) = mpsc::unbounded_channel();
        let (offer_tx, offer_rx) = mpsc::unbounded_channel();
        agent.register_peer(room, id, server_signal_tx).await;
        let offer = connection.create_offer(None).await.unwrap();
        connection
            .set_local_description(offer.clone())
            .await
            .unwrap();
        let answer = agent
            .process_signal(
                &SignalMessage {
                    kind: "offer".to_owned(),
                    room: room.to_owned(),
                    peer_id: id.to_owned(),
                    from: id.to_owned(),
                    to: "server".to_owned(),
                    sdp: offer.sdp,
                    candidate: None,
                },
                room,
                id,
            )
            .await
            .unwrap()
            .unwrap();
        connection
            .set_remote_description(RTCSessionDescription::answer(answer.sdp).unwrap())
            .await
            .unwrap();

        let browser_agent = agent.clone();
        let browser_room = room.to_owned();
        let browser_id = id.to_owned();
        let browser_to_server = tokio::spawn(async move {
            while let Some(candidate) = candidate_rx.recv().await {
                let signal = SignalMessage {
                    kind: "candidate".to_owned(),
                    room: browser_room.clone(),
                    peer_id: browser_id.clone(),
                    from: browser_id.clone(),
                    to: "server".to_owned(),
                    sdp: String::new(),
                    candidate: Some(CandidateMessage {
                        candidate: candidate.candidate,
                        sdp_mid: candidate.sdp_mid,
                        sdp_mline_index: candidate.sdp_mline_index,
                    }),
                };
                let _ = browser_agent
                    .process_signal(&signal, &browser_room, &browser_id)
                    .await;
            }
        });
        let browser_connection = connection.clone();
        let server_to_browser = tokio::spawn(async move {
            while let Some(payload) = server_signal_rx.recv().await {
                let Ok(signal) = serde_json::from_str::<SignalMessage>(&payload) else {
                    continue;
                };
                match signal.kind.as_str() {
                    "candidate" => {
                        if let Some(candidate) = signal.candidate {
                            let _ = browser_connection
                                .add_ice_candidate(RTCIceCandidateInit {
                                    candidate: candidate.candidate,
                                    sdp_mid: candidate.sdp_mid,
                                    sdp_mline_index: candidate.sdp_mline_index,
                                    username_fragment: None,
                                    url: None,
                                })
                                .await;
                        }
                    }
                    "offer" => {
                        let _ = offer_tx.send(signal);
                    }
                    _ => {}
                }
            }
        });

        let (packet_tx, packet_rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            while let Some(track) = track_rx.recv().await {
                while let Some(event) = track.poll().await {
                    if let TrackRemoteEvent::OnRtpPacket(packet) = event {
                        if packet_tx.send(packet).is_err() {
                            return;
                        }
                    }
                }
            }
        });

        TestPeer {
            id: id.to_owned(),
            connection,
            offer_rx,
            packet_rx,
            connected_rx,
            forwarding_tasks: vec![browser_to_server, server_to_browser],
        }
    }

    async fn answer_server_offer(agent: &WebRtcAgent, room: &str, peer: &mut TestPeer) {
        let offer = timeout(Duration::from_secs(10), peer.offer_rx.recv())
            .await
            .expect("timed out waiting for server renegotiation offer")
            .expect("server offer channel closed");
        peer.connection
            .set_remote_description(RTCSessionDescription::offer(offer.sdp).unwrap())
            .await
            .unwrap();
        let answer = peer.connection.create_answer(None).await.unwrap();
        peer.connection
            .set_local_description(answer.clone())
            .await
            .unwrap();
        agent
            .process_signal(
                &SignalMessage {
                    kind: "answer".to_owned(),
                    room: room.to_owned(),
                    peer_id: peer.id.clone(),
                    from: peer.id.clone(),
                    to: "server".to_owned(),
                    sdp: answer.sdp,
                    candidate: None,
                },
                room,
                &peer.id,
            )
            .await
            .unwrap();
    }

    async fn send_test_rtp(track: Arc<TrackLocalStaticRTP>) {
        let mut sequence_number = 1u16;
        loop {
            let mut packet = Packet::default();
            packet.header.version = 2;
            packet.header.payload_type = 96;
            packet.header.ssrc = 1234;
            packet.header.sequence_number = sequence_number;
            packet.header.timestamp = u32::from(sequence_number) * 3_000;
            packet.payload = vec![0xaa, 0xbb, 0xcc, 0xdd].into();
            if track.write_rtp(packet).await.is_err() {
                return;
            }
            sequence_number = sequence_number.wrapping_add(1);
            sleep(Duration::from_millis(20)).await;
        }
    }

    const BASELINE_PACKET_RATE: u64 = 50;
    const BASELINE_PAYLOAD_SIZE: usize = 1200;
    const BASELINE_DURATION: Duration = Duration::from_secs(10);

    #[derive(Clone, Copy)]
    struct ProcessUsage {
        user_microseconds: i64,
        system_microseconds: i64,
        peak_rss_kilobytes: i64,
    }

    fn process_usage() -> ProcessUsage {
        let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
        let result = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
        assert_eq!(result, 0, "getrusage failed");
        let usage = unsafe { usage.assume_init() };
        ProcessUsage {
            user_microseconds: usage.ru_utime.tv_sec * 1_000_000 + usage.ru_utime.tv_usec,
            system_microseconds: usage.ru_stime.tv_sec * 1_000_000 + usage.ru_stime.tv_usec,
            peak_rss_kilobytes: usage.ru_maxrss,
        }
    }

    fn print_host_facts() {
        let cpu = std::fs::read_to_string("/proc/cpuinfo")
            .ok()
            .and_then(|contents| {
                contents.lines().find_map(|line| {
                    line.strip_prefix("model name\t: ")
                        .or_else(|| line.strip_prefix("Hardware\t: "))
                        .map(str::to_owned)
                })
            })
            .unwrap_or_else(|| "unknown".to_owned());
        let memory_kilobytes = std::fs::read_to_string("/proc/meminfo")
            .ok()
            .and_then(|contents| {
                contents.lines().find_map(|line| {
                    line.strip_prefix("MemTotal:")
                        .and_then(|value| value.split_whitespace().next())
                        .and_then(|value| value.parse::<u64>().ok())
                })
            })
            .unwrap_or_default();
        let kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease")
            .unwrap_or_else(|_| "unknown\n".to_owned());
        eprintln!(
            "baseline_host arch={} kernel={} cpu={cpu} memory_total_kb={memory_kilobytes}",
            std::env::consts::ARCH,
            kernel.trim()
        );
    }

    async fn send_baseline_rtp(
        track: Arc<TrackLocalStaticRTP>,
        duration: Duration,
        packet_rate: u64,
        payload_size: usize,
        sent_packets: Arc<AtomicU64>,
    ) {
        let mut ticker = tokio::time::interval(Duration::from_secs_f64(1.0 / packet_rate as f64));
        let deadline = Instant::now() + duration;
        let mut sequence_number = 1u16;
        loop {
            ticker.tick().await;
            if Instant::now() >= deadline {
                break;
            }
            let mut packet = Packet::default();
            packet.header.version = 2;
            packet.header.payload_type = 96;
            packet.header.ssrc = 1234;
            packet.header.sequence_number = sequence_number;
            packet.header.timestamp = u32::from(sequence_number) * 1_800;
            packet.payload = vec![0xaa; payload_size].into();
            if track.write_rtp(packet).await.is_err() {
                break;
            }
            sent_packets.fetch_add(1, Ordering::Relaxed);
            sequence_number = sequence_number.wrapping_add(1);
        }
    }

    async fn run_resource_baseline(participant_count: usize) {
        assert!(participant_count == 2 || participant_count == 3);
        print_host_facts();
        let usage_before = process_usage();
        let started = Instant::now();
        let agent = WebRtcAgent::with_udp_addrs(vec!["127.0.0.1:0".to_owned()]);
        let room = format!("resource-baseline-{participant_count}");
        let source_track = create_video_track();
        let mut peers =
            vec![connect_test_peer(&agent, &room, "a", Some(source_track.clone()), false).await];
        let mut peer_b = connect_test_peer(&agent, &room, "b", None, true).await;
        timeout(Duration::from_secs(10), peers[0].connected_rx.recv())
            .await
            .expect("publisher did not establish loopback ICE")
            .expect("publisher ICE state stream closed");
        timeout(Duration::from_secs(10), peer_b.connected_rx.recv())
            .await
            .expect("subscriber B did not establish loopback ICE")
            .expect("subscriber B ICE state stream closed");
        let warmup_sender = tokio::spawn(send_test_rtp(source_track.clone()));
        timeout(Duration::from_secs(10), async {
            loop {
                if agent
                    .published_tracks
                    .lock()
                    .await
                    .values()
                    .any(|publication| publication.source_peer_key == format!("{room}:a"))
                {
                    break;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("publisher track was not published to the server");
        warmup_sender.abort();
        answer_server_offer(&agent, &room, &mut peer_b).await;
        while peer_b.packet_rx.try_recv().is_ok() {}
        peers.push(peer_b);

        if participant_count == 3 {
            let mut peer_c = connect_test_peer(&agent, &room, "c", None, true).await;
            timeout(Duration::from_secs(10), peer_c.connected_rx.recv())
                .await
                .expect("subscriber C did not establish loopback ICE")
                .expect("subscriber C ICE state stream closed");
            peers.push(peer_c);
        }

        let sent_packets = Arc::new(AtomicU64::new(0));
        let sender = tokio::spawn(send_baseline_rtp(
            source_track,
            BASELINE_DURATION,
            BASELINE_PACKET_RATE,
            BASELINE_PAYLOAD_SIZE,
            sent_packets.clone(),
        ));
        timeout(BASELINE_DURATION + Duration::from_secs(2), async {
            while sent_packets.load(Ordering::Relaxed) < BASELINE_PACKET_RATE * 9 {
                sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("synthetic sender did not produce the expected workload");
        sender.await.expect("synthetic sender task panicked");

        let sent_count = sent_packets.load(Ordering::Relaxed);
        let minimum_received = sent_count * 90 / 100;
        let mut received_counts = Vec::new();
        for peer in peers.iter_mut().skip(1) {
            let mut received = 0u64;
            while let Ok(packet) = peer.packet_rx.try_recv() {
                assert_eq!(packet.payload.len(), BASELINE_PAYLOAD_SIZE);
                received += 1;
            }
            assert!(
                received >= minimum_received,
                "subscriber {} received {received}/{sent_count} RTP packets",
                peer.id
            );
            received_counts.push((peer.id.clone(), received));
        }

        for peer in &peers {
            for task in &peer.forwarding_tasks {
                task.abort();
            }
            let _ = peer.connection.close().await;
            agent.remove_peer(&room, &peer.id).await;
        }
        timeout(Duration::from_secs(2), async {
            loop {
                let relay_tasks_empty = agent.relay_tasks.lock().await.is_empty();
                let publications_empty = agent.published_tracks.lock().await.is_empty();
                if relay_tasks_empty && publications_empty {
                    break;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("relay tasks or publications remained after teardown");
        assert!(agent.connections.lock().await.is_empty());
        assert!(agent.senders.lock().await.is_empty());
        assert!(agent.relay_tasks.lock().await.is_empty());
        assert!(agent.published_tracks.lock().await.is_empty());
        assert!(agent.relays.lock().await.is_empty());

        let elapsed = started.elapsed();
        let usage_after = process_usage();
        eprintln!(
            "baseline_result participants={participant_count} packet_rate={BASELINE_PACKET_RATE}/s payload_bytes={BASELINE_PAYLOAD_SIZE} duration_s={} sent_packets={sent_count} received_packets={received_counts:?} elapsed_s={:.3} cpu_user_s={:.3} cpu_system_s={:.3} peak_rss_mib={:.2}",
            BASELINE_DURATION.as_secs(),
            elapsed.as_secs_f64(),
            (usage_after.user_microseconds - usage_before.user_microseconds) as f64 / 1_000_000.0,
            (usage_after.system_microseconds - usage_before.system_microseconds) as f64
                / 1_000_000.0,
            usage_after.peak_rss_kilobytes as f64 / 1024.0
        );
    }

    #[tokio::test]
    #[ignore = "resource baseline; run with --ignored --exact webrtc::tests::resource_baseline_two_peers --nocapture"]
    async fn resource_baseline_two_peers() {
        run_resource_baseline(2).await;
    }

    #[tokio::test]
    #[ignore = "resource baseline; run with --ignored --exact webrtc::tests::resource_baseline_three_peers --nocapture"]
    async fn resource_baseline_three_peers() {
        run_resource_baseline(3).await;
    }

    async fn create_browser_peer() -> Arc<dyn PeerConnection> {
        Arc::new(
            PeerConnectionBuilder::new()
                .with_handler(Arc::new(NoopHandler))
                .with_udp_addrs(vec!["127.0.0.1:0"])
                .build()
                .await
                .unwrap(),
        )
    }

    async fn negotiate_initial_offer(agent: &WebRtcAgent, browser: &Arc<dyn PeerConnection>) {
        browser
            .create_data_channel("negotiation-check", None)
            .await
            .unwrap();
        let offer = browser.create_offer(None).await.unwrap();
        browser.set_local_description(offer.clone()).await.unwrap();
        let answer = agent
            .process_signal(
                &SignalMessage {
                    kind: "offer".to_owned(),
                    room: "negotiation-room".to_owned(),
                    peer_id: "browser".to_owned(),
                    from: "browser".to_owned(),
                    to: "server".to_owned(),
                    sdp: offer.sdp,
                    candidate: None,
                },
                "negotiation-room",
                "browser",
            )
            .await
            .unwrap()
            .unwrap();
        browser
            .set_remote_description(RTCSessionDescription::answer(answer.sdp).unwrap())
            .await
            .unwrap();
    }

    async fn next_signal_of_kind(
        receiver: &mut tokio::sync::mpsc::UnboundedReceiver<String>,
        expected_kind: &str,
    ) -> SignalMessage {
        loop {
            let payload = receiver.recv().await.unwrap();
            let signal: SignalMessage = serde_json::from_str(&payload).unwrap();
            if signal.kind == expected_kind {
                return signal;
            }
        }
    }

    #[test]
    fn relay_destination_selection_includes_initializing_room_peers() {
        let connections = ["room-a:publisher", "room-a:joining", "room-b:other"];
        let destinations = connections
            .into_iter()
            .filter(|key| is_relay_destination(key, "room-a:publisher", "room-a"))
            .collect::<Vec<_>>();

        assert_eq!(destinations, ["room-a:joining"]);
    }

    #[test]
    fn destination_codec_check_matches_mime_rate_and_channels() {
        let offer = "v=0\r\na=rtpmap:111 opus/48000/2\r\na=rtpmap:96 VP8/90000\r\n";

        assert!(remote_sdp_supports_codec(offer, "audio/opus", 48_000, 2));
        assert!(remote_sdp_supports_codec(offer, "video/VP8", 90_000, 0));
        assert!(!remote_sdp_supports_codec(offer, "audio/opus", 48_000, 1));
        assert!(!remote_sdp_supports_codec(offer, "video/H264", 90_000, 0));
    }

    #[tokio::test]
    async fn relay_errors_are_signaled_to_the_affected_peer() {
        let agent = WebRtcAgent::new();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        agent.register_peer("relay-room", "subscriber", tx).await;

        agent
            .notify_peer_relay_error("relay-room:subscriber", "RTP write failed")
            .await;
        let signal: SignalMessage = serde_json::from_str(&rx.recv().await.unwrap()).unwrap();

        assert_eq!(signal.kind, "relay-error");
        assert_eq!(signal.to, "subscriber");
        assert_eq!(signal.sdp, "RTP write failed");
    }

    #[tokio::test]
    async fn minimal_data_channel_offer_receives_a_server_answer() {
        let agent = WebRtcAgent::new();
        let browser = create_browser_peer().await;
        negotiate_initial_offer(&agent, &browser).await;
        agent.remove_peer("negotiation-room", "browser").await;
        browser.close().await.unwrap();
    }

    #[tokio::test]
    async fn three_peer_rtp_replay_and_publisher_cleanup() {
        let agent = WebRtcAgent::with_udp_addrs(vec!["127.0.0.1:0".to_owned()]);
        let room = "three-peer-rtp-room";
        let source_track = create_video_track();
        let mut peer_a =
            connect_test_peer(&agent, room, "a", Some(source_track.clone()), false).await;
        let mut peer_b = connect_test_peer(&agent, room, "b", None, true).await;
        timeout(Duration::from_secs(10), peer_a.connected_rx.recv())
            .await
            .expect("peer A did not connect")
            .expect("peer A ICE state stream closed");
        timeout(Duration::from_secs(10), peer_b.connected_rx.recv())
            .await
            .expect("peer B did not connect")
            .expect("peer B ICE state stream closed");
        let source_sender = peer_a.connection.get_senders().await.remove(0);
        assert_eq!(
            source_sender.get_parameters().await.unwrap().encodings[0]
                .rtp_coding_parameters
                .ssrc,
            Some(1234)
        );
        let sender = tokio::spawn(send_test_rtp(source_track));

        timeout(Duration::from_secs(10), async {
            loop {
                if agent
                    .published_tracks
                    .lock()
                    .await
                    .values()
                    .any(|publication| publication.source_peer_key == format!("{room}:a"))
                {
                    break;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("publisher track was not published to the server");

        answer_server_offer(&agent, room, &mut peer_b).await;
        let packet_b = timeout(Duration::from_secs(10), peer_b.packet_rx.recv())
            .await
            .expect("peer B did not receive relayed RTP")
            .expect("peer B RTP stream closed");
        assert_eq!(packet_b.payload.as_ref(), [0xaa, 0xbb, 0xcc, 0xdd]);

        let mut peer_c = connect_test_peer(&agent, room, "c", None, true).await;
        timeout(Duration::from_secs(10), peer_c.connected_rx.recv())
            .await
            .expect("peer C did not connect")
            .expect("peer C ICE state stream closed");
        let packet_c = timeout(Duration::from_secs(10), peer_c.packet_rx.recv())
            .await
            .expect("late-joining peer C did not receive replayed RTP")
            .expect("peer C RTP stream closed");
        assert_eq!(packet_c.payload.as_ref(), [0xaa, 0xbb, 0xcc, 0xdd]);

        let publication_key = agent
            .published_tracks
            .lock()
            .await
            .keys()
            .find(|key| key.starts_with(&format!("{room}:a:")))
            .cloned()
            .expect("publisher publication missing before disconnect");
        sender.abort();
        agent.remove_peer(room, "a").await;

        assert!(
            !agent
                .published_tracks
                .lock()
                .await
                .contains_key(&publication_key)
        );
        assert!(
            !agent
                .relay_tasks
                .lock()
                .await
                .contains_key(&publication_key)
        );
        assert!(
            !agent
                .relays
                .lock()
                .await
                .keys()
                .any(|key| key.starts_with(&format!("{publication_key}->")))
        );

        answer_server_offer(&agent, room, &mut peer_b).await;
        answer_server_offer(&agent, room, &mut peer_c).await;

        for peer in [&mut peer_b, &mut peer_c] {
            while peer.packet_rx.try_recv().is_ok() {}
            assert!(
                timeout(Duration::from_millis(200), peer.packet_rx.recv())
                    .await
                    .is_err()
            );
        }

        for peer in [&peer_a, &peer_b, &peer_c] {
            for task in &peer.forwarding_tasks {
                task.abort();
            }
            let _ = peer.connection.close().await;
            agent.remove_peer(room, &peer.id).await;
        }
    }

    #[tokio::test]
    async fn concurrent_track_changes_serialize_and_queue_renegotiation() {
        let agent = WebRtcAgent::new();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        agent.register_peer("negotiation-room", "browser", tx).await;
        let browser = create_browser_peer().await;
        negotiate_initial_offer(&agent, &browser).await;

        let key = agent.key("negotiation-room", "browser");
        let audio_agent = agent.clone();
        let audio_key = key.clone();
        let audio_change = async move {
            let lock = audio_agent.negotiation_lock(&audio_key).await;
            let _guard = lock.lock().await;
            let server = audio_agent.get(&audio_key).await?;
            server
                .add_transceiver_from_kind(RtpCodecKind::Audio, None)
                .await?;
            audio_agent
                .request_renegotiation_locked(&audio_key, "negotiation-room", "browser", &server)
                .await
        };
        let video_agent = agent.clone();
        let video_key = key.clone();
        let video_change = async move {
            let lock = video_agent.negotiation_lock(&video_key).await;
            let _guard = lock.lock().await;
            let server = video_agent.get(&video_key).await?;
            server
                .add_transceiver_from_kind(RtpCodecKind::Video, None)
                .await?;
            video_agent
                .request_renegotiation_locked(&video_key, "negotiation-room", "browser", &server)
                .await
        };
        let (audio_result, video_result) = tokio::join!(audio_change, video_change);
        audio_result.unwrap();
        video_result.unwrap();
        assert!(agent.local_offers.lock().await.contains(&key));
        assert!(agent.queued_negotiations.lock().await.contains(&key));

        let offer_message = next_signal_of_kind(&mut rx, "offer").await;
        browser
            .set_remote_description(RTCSessionDescription::offer(offer_message.sdp).unwrap())
            .await
            .unwrap();
        let answer = browser.create_answer(None).await.unwrap();
        browser.set_local_description(answer.clone()).await.unwrap();

        assert!(
            agent
                .process_signal(
                    &SignalMessage {
                        kind: "answer".to_owned(),
                        room: "negotiation-room".to_owned(),
                        peer_id: "browser".to_owned(),
                        from: "browser".to_owned(),
                        to: "server".to_owned(),
                        sdp: answer.sdp,
                        candidate: None,
                    },
                    "negotiation-room",
                    "browser",
                )
                .await
                .unwrap()
                .is_none()
        );

        let queued_offer = next_signal_of_kind(&mut rx, "offer").await;
        assert_eq!(queued_offer.to, "browser");
        assert!(agent.local_offers.lock().await.contains(&key));

        agent.remove_peer("negotiation-room", "browser").await;
        browser.close().await.unwrap();
    }

    #[tokio::test]
    async fn incoming_offer_rolls_back_a_colliding_server_offer() {
        let agent = WebRtcAgent::new();
        let browser = create_browser_peer().await;
        negotiate_initial_offer(&agent, &browser).await;

        let key = agent.key("negotiation-room", "browser");
        let server = agent.get(&key).await.unwrap();
        server
            .add_transceiver_from_kind(RtpCodecKind::Audio, None)
            .await
            .unwrap();
        agent
            .send_offer(&key, "negotiation-room", "browser", &server)
            .await
            .unwrap();
        browser
            .create_data_channel("collision-check", None)
            .await
            .unwrap();
        let browser_offer = browser.create_offer(None).await.unwrap();
        browser
            .set_local_description(browser_offer.clone())
            .await
            .unwrap();

        let answer = agent
            .process_signal(
                &SignalMessage {
                    kind: "offer".to_owned(),
                    room: "negotiation-room".to_owned(),
                    peer_id: "browser".to_owned(),
                    from: "browser".to_owned(),
                    to: "server".to_owned(),
                    sdp: browser_offer.sdp,
                    candidate: None,
                },
                "negotiation-room",
                "browser",
            )
            .await
            .unwrap()
            .unwrap();

        assert_eq!(answer.kind, "answer");
        assert!(!agent.local_offers.lock().await.contains(&key));
        browser
            .set_remote_description(RTCSessionDescription::answer(answer.sdp).unwrap())
            .await
            .unwrap();
        agent.remove_peer("negotiation-room", "browser").await;
        browser.close().await.unwrap();
    }
}
