use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use async_trait::async_trait;
use rtc::{
    media_stream::MediaStreamTrack,
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
        Self::default()
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
                        let Some(TrackRemoteEvent::OnRtpPacket(packet)) = event else { break };
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
        let pc = PeerConnectionBuilder::new()
            .with_configuration(config)
            .with_handler(Arc::new(PeerConnectionHandler {
                peer_key: key.to_owned(),
                agent: self.clone(),
            }))
            .with_udp_addrs(vec!["0.0.0.0:0"])
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
    use super::{WebRtcAgent, is_relay_destination, remote_sdp_supports_codec};
    use crate::signaling::SignalMessage;
    use async_trait::async_trait;
    use rtc::rtp_transceiver::rtp_sender::RtpCodecKind;
    use std::sync::Arc;
    use webrtc::peer_connection::{
        PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler,
        RTCPeerConnectionIceEvent, RTCSessionDescription,
    };

    struct NoopHandler;

    #[async_trait]
    impl PeerConnectionEventHandler for NoopHandler {
        async fn on_ice_candidate(&self, _event: RTCPeerConnectionIceEvent) {}
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
