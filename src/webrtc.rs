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

use crate::metrics::MetricsRecorder;
use crate::signaling::{CandidateMessage, SignalMessage};
use rtc::rtp_transceiver::rtp_sender::RtpCodecKind;

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
    queued_negotiations: Arc<Mutex<HashMap<String, u32>>>,
    relay_lifecycle_lock: Arc<Mutex<()>>,
    udp_addrs: Arc<Vec<String>>,
}

struct PublishedTrack {
    source_peer_key: String,
    track: Arc<dyn TrackRemote + Send + Sync + 'static>,
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
                    display_name: String::new(),
                    role: String::new(),
                    muted: false,
                    camera_on: false,
                    screen_share: false,
                    sdp: String::new(),
                    candidate: Some(CandidateMessage {
                        candidate: candidate.candidate,
                        sdp_mid: candidate.sdp_mid,
                        sdp_mline_index: candidate.sdp_mline_index,
                    }),
                    participants: Vec::new(),
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
                let added_existing_tracks = self.add_existing_tracks(&key, room).await?;
                let answer = pc.create_answer(None).await?;
                pc.set_local_description(answer.clone()).await?;
                self.ready_peers.lock().await.insert(key.clone());
                if added_existing_tracks {
                    let mut queued = self.queued_negotiations.lock().await;
                    *queued.entry(key.clone()).or_insert(0) += 1;
                } else {
                    let mut queued = self.queued_negotiations.lock().await;
                    if let Some(count) = queued.get_mut(&key) {
                        *count -= 1;
                        if *count == 0 {
                            queued.remove(&key);
                        }
                    }
                }
                Ok(Some(SignalMessage {
                    kind: "answer".to_owned(),
                    room: room.to_owned(),
                    peer_id: peer_id.to_owned(),
                    from: "server".to_owned(),
                    to: peer_id.to_owned(),
                    display_name: String::new(),
                    role: String::new(),
                    muted: false,
                    camera_on: false,
                    screen_share: false,
                    sdp: answer.sdp,
                    candidate: None,
                    participants: Vec::new(),
                }))
            }
            "answer" => {
                let pc = self.get(&key).await?;
                pc.set_remote_description(RTCSessionDescription::answer(signal.sdp.clone())?)
                    .await?;
                let had_local_offer = self.local_offers.lock().await.remove(&key);
                if had_local_offer {
                    let mut send_next = false;
                    {
                        let mut queued = self.queued_negotiations.lock().await;
                        if let Some(count) = queued.get_mut(&key) {
                            *count -= 1;
                            if *count == 0 {
                                queued.remove(&key);
                            } else {
                                send_next = true;
                            }
                        }
                    }
                    if send_next {
                        self.send_offer(&key, room, peer_id, &pc).await?;
                    }
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

    pub async fn flush_queued_negotiation(
        &self,
        room: &str,
        peer_id: &str,
    ) -> Result<(), webrtc::error::Error> {
        let key = self.key(room, peer_id);
        let negotiation_lock = self.negotiation_lock(&key).await;
        let _negotiation_guard = negotiation_lock.lock().await;
        let mut queued = self.queued_negotiations.lock().await;
        if let Some(count) = queued.get_mut(&key) {
            *count -= 1;
            if *count == 0 {
                queued.remove(&key);
            }
        } else {
            return Ok(());
        }
        drop(queued);
        let pc = self.get(&key).await?;
        self.send_offer(&key, room, peer_id, &pc).await
    }

    async fn relay_track(
        &self,
        source_key: String,
        source: Arc<dyn TrackRemote + Send + Sync + 'static>,
    ) -> Result<(), webrtc::error::Error> {
        let lifecycle_guard = self.relay_lifecycle_lock.lock().await;
        if !self.connections.lock().await.contains_key(&source_key) {
            return Ok(());
        }
        // Extract room as owned String to avoid lifetime issues
        let room = source_key.split_once(':').map(|(r, _)| r.to_owned());
        let Some(room) = room else {
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
            .filter(|key| is_relay_destination(key, &source_key, &room))
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
                eprintln!("relay destination not ready: {destination_key} for {publication_key}");
                continue;
            }
            self.add_relay_track(&publication_key, &source_key, &destination_key, &source)
                .await?;

            let destination_peer = destination_key.split(':').nth(1).unwrap_or_default();
            self.request_renegotiation_locked(&destination_key, &room, destination_peer, &pc)
                .await?;
        }

        // Clone values needed for the spawned task to satisfy 'static lifetime
        let agent = self.clone();
        let source_for_task = source.clone();
        let publication_key_for_task = publication_key.clone();
        let room_for_task = room.clone();
        let (cancel_tx, mut cancel_rx) = oneshot::channel();
        self.relay_tasks
            .lock()
            .await
            .insert(publication_key.clone(), cancel_tx);
        MetricsRecorder::relay_task_started();
        tokio::spawn(async move {
            let mut failed_destinations = HashSet::new();
            let mut _packet_count = 0u64;
            loop {
                tokio::select! {
                    _ = &mut cancel_rx => break,
                    event = source_for_task.poll() => {
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
                        _packet_count += 1;
                        let outputs: Vec<_> = agent
                            .relays
                            .lock()
                            .await
                            .iter()
                            .filter(|(relay_key, _)| {
                                relay_key.starts_with(&format!("{publication_key_for_task}->"))
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
                            let output_ssrc = output.ssrcs().await.first().copied();
                            if let Some(ssrc) = output_ssrc {
                                packet.header.ssrc = ssrc;
                            }
                            let payload_len = packet.payload.len();
                            let kind = if packet.header.payload_type == 111 { "audio" } else { "video" };
                            if let Err(error) = output.write_rtp(packet).await {
                                MetricsRecorder::rtp_packet_dropped(&room_for_task, "write_error");
                                eprintln!("  write_rtp ERROR for {}: {}", destination_key, error);
                                if failed_destinations.insert(destination_key.clone()) {
                                    agent
                                        .notify_peer_relay_error(
                                            &destination_key,
                                            &error.to_string(),
                                        )
                                        .await;
                                }
                            } else {
                                MetricsRecorder::rtp_packet_forwarded(&room, kind, payload_len);
                            }
                        }
                    }
                }
            }
            MetricsRecorder::relay_task_stopped();
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
    ) -> Result<bool, webrtc::error::Error> {
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
        let mut added_tracks = false;

        for (publication_key, source_peer_key, source) in publications {
            added_tracks |= self
                .add_relay_track(&publication_key, &source_peer_key, destination_key, &source)
                .await?;
        }
        Ok(added_tracks)
    }

    async fn add_relay_track(
        &self,
        publication_key: &str,
        source_peer_key: &str,
        destination_key: &str,
        source: &Arc<dyn TrackRemote + Send + Sync + 'static>,
    ) -> Result<bool, webrtc::error::Error> {
        let relay_key = format!("{publication_key}->{destination_key}");
        if self.relays.lock().await.contains_key(&relay_key) {
            return Ok(false);
        }

        let kind = source.kind().await;
        eprintln!(
            "add_relay_track: kind={:?}, publication_key={}",
            kind, publication_key
        );
        let mut codec = None;
        for ssrc in source.ssrcs().await {
            if let Some(value) = source.codec(ssrc).await {
                eprintln!(
                    "  source codec for ssrc={}: mime_type={}, clock_rate={}, channels={}",
                    ssrc, value.mime_type, value.clock_rate, value.channels
                );
                codec = Some(value);
                break;
            }
        }
        let Some(codec) = codec else {
            eprintln!("  no codec found for source track");
            return Ok(false);
        };
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
            return Ok(false);
        };
        let Some((room, source_peer_id)) = source_peer_key.split_once(':') else {
            return Ok(false);
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
        // Determine track kind for metrics
        let track_kind = match kind {
            RtpCodecKind::Audio => "audio",
            RtpCodecKind::Video => "video",
            _ => "unknown",
        };
        MetricsRecorder::track_published(room, track_kind);
        Ok(true)
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
                display_name: String::new(),
                role: String::new(),
                muted: false,
                camera_on: false,
                screen_share: false,
                sdp: error.to_owned(),
                candidate: None,
                participants: Vec::new(),
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
                display_name: String::new(),
                role: String::new(),
                muted: false,
                camera_on: false,
                screen_share: false,
                sdp: offer.sdp,
                candidate: None,
                participants: Vec::new(),
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
        let has_local_offer = self.local_offers.lock().await.contains(key);
        if has_local_offer {
            let mut queued = self.queued_negotiations.lock().await;
            *queued.entry(key.to_owned()).or_insert(0) += 1;
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
        // Extract room from publication_key (format: room:peer:track_id)
        let room = publication_key.split(':').next().unwrap_or("");
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
            MetricsRecorder::track_unpublished(room);
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

    // Resource measurement helper using getrusage
    #[derive(Debug, Clone, Copy)]
    struct ResourceSnapshot {
        user_cpu_secs: f64,
        sys_cpu_secs: f64,
        max_rss_kb: i64,
    }

    fn snapshot_resources() -> ResourceSnapshot {
        let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
        // RUSAGE_SELF = 0
        unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
        ResourceSnapshot {
            user_cpu_secs: usage.ru_utime.tv_sec as f64
                + usage.ru_utime.tv_usec as f64 / 1_000_000.0,
            sys_cpu_secs: usage.ru_stime.tv_sec as f64
                + usage.ru_stime.tv_usec as f64 / 1_000_000.0,
            max_rss_kb: usage.ru_maxrss,
        }
    }

    fn diff_resources(after: ResourceSnapshot, before: ResourceSnapshot) -> (f64, f64, i64) {
        (
            after.user_cpu_secs - before.user_cpu_secs,
            after.sys_cpu_secs - before.sys_cpu_secs,
            after.max_rss_kb.saturating_sub(before.max_rss_kb),
        )
    }
    use webrtc::media_stream::{
        Track,
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

    fn create_video_track(ssrc: u32) -> Arc<TrackLocalStaticRTP> {
        Arc::new(TrackLocalStaticRTP::new(MediaStreamTrack::new(
            "test-stream".to_owned(),
            "test-video".to_owned(),
            "loopback-video".to_owned(),
            RtpCodecKind::Video,
            vec![RTCRtpEncodingParameters {
                rtp_coding_parameters: RTCRtpCodingParameters {
                    ssrc: Some(ssrc),
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

    fn create_audio_track(ssrc: u32) -> Arc<TrackLocalStaticRTP> {
        Arc::new(TrackLocalStaticRTP::new(MediaStreamTrack::new(
            "test-stream".to_owned(),
            "test-audio".to_owned(),
            "loopback-audio".to_owned(),
            RtpCodecKind::Audio,
            vec![RTCRtpEncodingParameters {
                rtp_coding_parameters: RTCRtpCodingParameters {
                    ssrc: Some(ssrc),
                    ..Default::default()
                },
                active: true,
                codec: RTCRtpCodec {
                    mime_type: "audio/opus".to_owned(),
                    clock_rate: 48_000,
                    channels: 2,
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
        let (offer_tx, mut offer_rx) = mpsc::unbounded_channel();
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
                    display_name: String::new(),
                    role: String::new(),
                    muted: false,
                    camera_on: false,
                    screen_share: false,
                    sdp: offer.sdp,
                    candidate: None,
                    participants: Vec::new(),
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
                    display_name: String::new(),
                    role: String::new(),
                    muted: false,
                    camera_on: false,
                    screen_share: false,
                    sdp: String::new(),
                    candidate: Some(CandidateMessage {
                        candidate: candidate.candidate,
                        sdp_mid: candidate.sdp_mid,
                        sdp_mline_index: candidate.sdp_mline_index,
                    }),
                    participants: Vec::new(),
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
        eprintln!(
            "  answer_server_offer: waiting for offer for peer {}",
            peer.id
        );
        let offer = timeout(Duration::from_secs(10), peer.offer_rx.recv())
            .await
            .expect("timed out waiting for server renegotiation offer")
            .expect("server offer channel closed");
        eprintln!("  answer_server_offer: received offer for peer {}", peer.id);
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
                    display_name: String::new(),
                    role: String::new(),
                    muted: false,
                    camera_on: false,
                    screen_share: false,
                    sdp: answer.sdp,
                    candidate: None,
                    participants: Vec::new(),
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
        let source_track = create_video_track(1234);
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
                    display_name: String::new(),
                    role: String::new(),
                    muted: false,
                    camera_on: false,
                    screen_share: false,
                    sdp: offer.sdp,
                    candidate: None,
                    participants: Vec::new(),
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
        let source_track = create_video_track(1234);
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
        assert!(agent.queued_negotiations.lock().await.contains_key(&key));

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
                        display_name: String::new(),
                        role: String::new(),
                        muted: false,
                        camera_on: false,
                        screen_share: false,
                        sdp: answer.sdp,
                        candidate: None,
                        participants: Vec::new(),
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
                    display_name: String::new(),
                    role: String::new(),
                    muted: false,
                    camera_on: false,
                    screen_share: false,
                    sdp: browser_offer.sdp,
                    candidate: None,
                    participants: Vec::new(),
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

    struct BidirectionalPeer {
        id: String,
        connection: Arc<dyn PeerConnection>,
        offer_rx: mpsc::UnboundedReceiver<SignalMessage>,
        packet_rx: mpsc::UnboundedReceiver<Packet>,
        connected_rx: mpsc::UnboundedReceiver<()>,
        forwarding_tasks: Vec<JoinHandle<()>>,
        audio_track: Arc<TrackLocalStaticRTP>,
        video_track: Arc<TrackLocalStaticRTP>,
    }

    async fn connect_bidirectional_peer(
        agent: &WebRtcAgent,
        room: &str,
        id: &str,
        audio_ssrc: u32,
        video_ssrc: u32,
    ) -> BidirectionalPeer {
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

        // Create audio and video tracks
        let audio_track = create_audio_track(audio_ssrc);
        let video_track = create_video_track(video_ssrc);

        // Add local tracks FIRST so they create sendrecv transceivers in the initial offer
        connection
            .add_track(audio_track.clone() as Arc<dyn TrackLocal>)
            .await
            .unwrap();
        connection
            .add_track(video_track.clone() as Arc<dyn TrackLocal>)
            .await
            .unwrap();

        // Add recvonly transceivers for receiving from up to 3 other peers
        // We create 3 per kind (one for each other peer)
        for _ in 0..3 {
            connection
                .add_transceiver_from_kind(
                    RtpCodecKind::Audio,
                    Some(RTCRtpTransceiverInit {
                        direction: RTCRtpTransceiverDirection::Recvonly,
                        ..Default::default()
                    }),
                )
                .await
                .unwrap();
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
        let (offer_tx, mut offer_rx) = mpsc::unbounded_channel();
        agent.register_peer(room, id, server_signal_tx).await;

        // Initial offer includes local tracks (sendrecv) + recvonly for other peers
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
                    display_name: String::new(),
                    role: String::new(),
                    muted: false,
                    camera_on: false,
                    screen_share: false,
                    sdp: offer.sdp,
                    candidate: None,
                    participants: Vec::new(),
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
                    display_name: String::new(),
                    role: String::new(),
                    muted: false,
                    camera_on: false,
                    screen_share: false,
                    sdp: String::new(),
                    candidate: Some(CandidateMessage {
                        candidate: candidate.candidate,
                        sdp_mid: candidate.sdp_mid,
                        sdp_mline_index: candidate.sdp_mline_index,
                    }),
                    participants: Vec::new(),
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
        let peer_id = id.to_owned();
        tokio::spawn(async move {
            while let Some(track) = track_rx.recv().await {
                let track_id = track.track_id().await;
                let peer_id = peer_id.clone();
                let packet_tx = packet_tx.clone();
                tokio::spawn(async move {
                    let mut packet_count = 0u64;
                    while let Some(event) = track.poll().await {
                        if let TrackRemoteEvent::OnRtpPacket(packet) = event {
                            packet_count += 1;
                            if packet_tx.send(packet).is_err() {
                                return;
                            }
                        }
                    }
                });
            }
        });

        BidirectionalPeer {
            id: id.to_owned(),
            connection,
            offer_rx,
            packet_rx,
            connected_rx,
            forwarding_tasks: vec![browser_to_server, server_to_browser],
            audio_track,
            video_track,
        }
    }

    async fn answer_server_offer_bidir(
        agent: &WebRtcAgent,
        room: &str,
        peer: &mut BidirectionalPeer,
    ) {
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
                    display_name: String::new(),
                    role: String::new(),
                    muted: false,
                    camera_on: false,
                    screen_share: false,
                    sdp: answer.sdp,
                    candidate: None,
                    participants: Vec::new(),
                },
                room,
                &peer.id,
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn four_peer_all_publishers_acceptance() {
        let agent = WebRtcAgent::with_udp_addrs(vec!["127.0.0.1:0".to_owned()]);
        let room = "four-peer-all-publishers";

        // Resource measurement: baseline
        let baseline = snapshot_resources();

        // Connect 4 peers, each with unique SSRCs for audio and video
        let mut peer0 = connect_bidirectional_peer(&agent, room, "peer0", 1000, 2000).await;
        let mut peer1 = connect_bidirectional_peer(&agent, room, "peer1", 1001, 2001).await;
        let mut peer2 = connect_bidirectional_peer(&agent, room, "peer2", 1002, 2002).await;
        let mut peer3 = connect_bidirectional_peer(&agent, room, "peer3", 1003, 2003).await;

        let mut peers = [&mut peer0, &mut peer1, &mut peer2, &mut peer3];

        // Wait for all ICE connections to be established
        for peer in peers.iter_mut() {
            timeout(Duration::from_secs(10), peer.connected_rx.recv())
                .await
                .expect(&format!("{} did not connect", peer.id))
                .expect(&format!("{} ICE state stream closed", peer.id));
        }

        // Resource measurement: after ICE connected
        let after_ice = snapshot_resources();
        let (ice_user, ice_sys, ice_rss) = diff_resources(after_ice, baseline);
        eprintln!(
            "[resources] after ICE: user={:.3}s sys={:.3}s total={:.3}s rss_delta={}KB",
            ice_user,
            ice_sys,
            ice_user + ice_sys,
            ice_rss
        );

        // Start sending RTP from all peers continuously
        let sender_tasks: Vec<_> = peers
            .iter()
            .map(|peer| {
                let audio_track = peer.audio_track.clone();
                let video_track = peer.video_track.clone();
                tokio::spawn(async move {
                    // Get SSRCs from tracks
                    let audio_ssrc = audio_track.ssrcs().await.first().copied().unwrap_or(1000);
                    let video_ssrc = video_track.ssrcs().await.first().copied().unwrap_or(2000);
                    let mut seq = 1u16;
                    loop {
                        // Send audio
                        let mut audio_packet = Packet::default();
                        audio_packet.header.version = 2;
                        audio_packet.header.payload_type = 111;
                        audio_packet.header.ssrc = audio_ssrc;
                        audio_packet.header.sequence_number = seq;
                        audio_packet.header.timestamp = u32::from(seq) * 960;
                        audio_packet.payload = vec![0xaa; 160].into();
                        if audio_track.write_rtp(audio_packet).await.is_err() {
                            break;
                        }

                        // Send video
                        let mut video_packet = Packet::default();
                        video_packet.header.version = 2;
                        video_packet.header.payload_type = 96;
                        video_packet.header.ssrc = video_ssrc;
                        video_packet.header.sequence_number = seq;
                        video_packet.header.timestamp = u32::from(seq) * 3000;
                        video_packet.payload = vec![0xbb; 1200].into();
                        if video_track.write_rtp(video_packet).await.is_err() {
                            break;
                        }

                        seq = seq.wrapping_add(1);
                        sleep(Duration::from_millis(20)).await;
                    }
                })
            })
            .collect();

        // Wait for all tracks to be published
        timeout(Duration::from_secs(30), async {
            loop {
                let published_tracks = agent.published_tracks.lock().await;
                let published_count = published_tracks.len();
                if published_count >= 8 {
                    // 4 peers * 2 tracks each
                    break;
                }
                drop(published_tracks);
                sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .expect("not all tracks were published to the server");

        // Resource measurement: after all tracks published
        let after_published = snapshot_resources();
        let (pub_user, pub_sys, pub_rss) = diff_resources(after_published, after_ice);
        eprintln!(
            "[resources] after tracks published: user={:.3}s sys={:.3}s total={:.3}s rss_delta={}KB",
            pub_user,
            pub_sys,
            pub_user + pub_sys,
            pub_rss
        );

        // Answer all server renegotiation offers for all peers
        // Offers are sent as tracks are relayed; answer until no more arrive within timeout
        for peer in peers.iter_mut() {
            let mut offer_count = 0;
            loop {
                match timeout(Duration::from_secs(5), peer.offer_rx.recv()).await {
                    Ok(Some(offer)) => {
                        offer_count += 1;
                        eprintln!(
                            "Received offer {} for peer {}:\n{}",
                            offer_count, peer.id, offer.sdp
                        );
                        peer.connection
                            .set_remote_description(
                                RTCSessionDescription::offer(offer.sdp).unwrap(),
                            )
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
                                    display_name: String::new(),
                                    role: String::new(),
                                    muted: false,
                                    camera_on: false,
                                    screen_share: false,
                                    sdp: answer.sdp,
                                    candidate: None,
                                    participants: Vec::new(),
                                },
                                room,
                                &peer.id,
                            )
                            .await
                            .unwrap();
                    }
                    Ok(None) => {
                        break;
                    }
                    Err(_) => {
                        break;
                    }
                }
            }
        }

        // Resource measurement: after all offers answered
        let after_offers = snapshot_resources();
        let (off_user, off_sys, off_rss) = diff_resources(after_offers, after_published);
        eprintln!(
            "[resources] after offers answered: user={:.3}s sys={:.3}s total={:.3}s rss_delta={}KB",
            off_user,
            off_sys,
            off_user + off_sys,
            off_rss
        );

        // Verify each peer receives audio and video from all 3 other peers
        // Expected per peer: 3 remote peers × 50 pps × 10s = 1500 packets per media type
        const EXPECTED_PER_PEER_PER_MEDIA: u64 = 1500;
        const EXPECTED_TOTAL_PER_MEDIA: u64 = 4 * EXPECTED_PER_PEER_PER_MEDIA; // 4 peers × 1500 = 6000

        let mut per_peer_audio = Vec::new();
        let mut per_peer_video = Vec::new();

        for peer in peers.iter_mut() {
            let mut audio_received = 0u64;
            let mut video_received = 0u64;
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline {
                sleep(Duration::from_millis(10)).await;
            }
            // Drain packet channel for received packets
            while let Ok(packet) = peer.packet_rx.try_recv() {
                if packet.header.payload_type == 111 {
                    audio_received += 1;
                } else if packet.header.payload_type == 96 {
                    video_received += 1;
                }
            }
            per_peer_audio.push((peer.id.clone(), audio_received));
            per_peer_video.push((peer.id.clone(), video_received));

            // Each peer should receive from 3 other peers
            assert!(
                audio_received > 100,
                "peer {} received too few audio packets: {}",
                peer.id,
                audio_received
            );
            assert!(
                video_received > 100,
                "peer {} received too few video packets: {}",
                peer.id,
                video_received
            );
        }

        // Report delivery quality metrics
        eprintln!("[delivery] Per-peer audio received:");
        for (id, recv) in &per_peer_audio {
            let loss_pct = if *recv < EXPECTED_PER_PEER_PER_MEDIA {
                ((EXPECTED_PER_PEER_PER_MEDIA - *recv) as f64 / EXPECTED_PER_PEER_PER_MEDIA as f64)
                    * 100.0
            } else {
                0.0
            };
            let bitrate_kbps = (*recv as f64 * 160.0 * 8.0) / 10_000.0;
            eprintln!(
                "  {}: recv={}/{} loss={:.2}% bitrate={:.1}kbps",
                id, recv, EXPECTED_PER_PEER_PER_MEDIA, loss_pct, bitrate_kbps
            );
        }
        eprintln!("[delivery] Per-peer video received:");
        for (id, recv) in &per_peer_video {
            let loss_pct = if *recv < EXPECTED_PER_PEER_PER_MEDIA {
                ((EXPECTED_PER_PEER_PER_MEDIA - *recv) as f64 / EXPECTED_PER_PEER_PER_MEDIA as f64)
                    * 100.0
            } else {
                0.0
            };
            let bitrate_kbps = (*recv as f64 * 1200.0 * 8.0) / 10_000.0;
            eprintln!(
                "  {}: recv={}/{} loss={:.2}% bitrate={:.1}kbps",
                id, recv, EXPECTED_PER_PEER_PER_MEDIA, loss_pct, bitrate_kbps
            );
        }

        // Aggregate totals
        let total_audio: u64 = per_peer_audio.iter().map(|(_, v)| *v).sum();
        let total_video: u64 = per_peer_video.iter().map(|(_, v)| *v).sum();
        let total_audio_loss = if total_audio < EXPECTED_TOTAL_PER_MEDIA {
            ((EXPECTED_TOTAL_PER_MEDIA - total_audio) as f64 / EXPECTED_TOTAL_PER_MEDIA as f64)
                * 100.0
        } else {
            0.0
        };
        let total_video_loss = if total_video < EXPECTED_TOTAL_PER_MEDIA {
            ((EXPECTED_TOTAL_PER_MEDIA - total_video) as f64 / EXPECTED_TOTAL_PER_MEDIA as f64)
                * 100.0
        } else {
            0.0
        };
        eprintln!(
            "[delivery] TOTAL: audio={}/{} loss={:.2}% video={}/{} loss={:.2}%",
            total_audio,
            EXPECTED_TOTAL_PER_MEDIA,
            total_audio_loss,
            total_video,
            EXPECTED_TOTAL_PER_MEDIA,
            total_video_loss
        );

        // Resource measurement: after receive window (end of workload)
        let after_receive = snapshot_resources();
        let (recv_user, recv_sys, recv_rss) = diff_resources(after_receive, after_offers);
        eprintln!(
            "[resources] after receive window: user={:.3}s sys={:.3}s total={:.3}s rss_delta={}KB",
            recv_user,
            recv_sys,
            recv_user + recv_sys,
            recv_rss
        );

        // Cleanup
        for task in sender_tasks {
            task.abort();
        }
        for peer in peers.iter() {
            for task in &peer.forwarding_tasks {
                task.abort();
            }
            let _ = peer.connection.close().await;
            agent.remove_peer(room, &peer.id).await;
        }

        // Verify cleanup
        timeout(Duration::from_secs(5), async {
            loop {
                let relay_tasks_empty = agent.relay_tasks.lock().await.is_empty();
                let publications_empty = agent.published_tracks.lock().await.is_empty();
                let relays_empty = agent.relays.lock().await.is_empty();
                if relay_tasks_empty && publications_empty && relays_empty {
                    break;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("relay tasks, publications, or relays remained after teardown");

        // Resource measurement: after cleanup
        let after_cleanup = snapshot_resources();
        let (cleanup_user, cleanup_sys, cleanup_rss) = diff_resources(after_cleanup, after_receive);
        eprintln!(
            "[resources] after cleanup: user={:.3}s sys={:.3}s total={:.3}s rss_delta={}KB",
            cleanup_user,
            cleanup_sys,
            cleanup_user + cleanup_sys,
            cleanup_rss
        );

        // Total test resources
        let (total_user, total_sys, total_rss) = diff_resources(after_cleanup, baseline);
        eprintln!(
            "[resources] TOTAL: user={:.3}s sys={:.3}s total={:.3}s peak_rss={}KB",
            total_user,
            total_sys,
            total_user + total_sys,
            after_cleanup.max_rss_kb
        );

        assert!(agent.connections.lock().await.is_empty());
        assert!(agent.senders.lock().await.is_empty());
    }
}
