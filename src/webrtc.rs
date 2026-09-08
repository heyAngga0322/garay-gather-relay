use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use rtc::{
    media_stream::MediaStreamTrack,
    rtp_transceiver::rtp_sender::{RTCRtpCodingParameters, RTCRtpEncodingParameters},
};
use tokio::sync::{mpsc, Mutex};
use webrtc::{
    media_stream::{
        track_local::{static_rtp::TrackLocalStaticRTP, TrackLocal},
        track_remote::{TrackRemote, TrackRemoteEvent},
        Track,
    },
    peer_connection::{
        PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler,
        RTCConfigurationBuilder, RTCIceCandidateInit, RTCIceServer,
        RTCSessionDescription, RTCPeerConnectionIceEvent,
    },
};

use crate::signaling::{CandidateMessage, SignalMessage};

#[derive(Default)]
pub struct WebRtcAgent {
    connections: Arc<Mutex<HashMap<String, Arc<dyn PeerConnection>>>>,
    senders: Arc<Mutex<HashMap<String, mpsc::UnboundedSender<String>>>>,
    relays: Arc<Mutex<HashMap<String, Arc<TrackLocalStaticRTP>>>>,
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
        let Ok(candidate) = candidate.to_json() else { return };
        let Some((room, peer_id)) = self.peer_key.split_once(':') else { return };
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
            if let Err(error) = agent.relay_track(source_key, track).await {
                eprintln!("RTP relay error: {error}");
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
        self.senders.lock().await.insert(self.key(room, peer_id), tx);
    }

    pub async fn process_signal(
        &self,
        signal: &SignalMessage,
        room: &str,
        peer_id: &str,
    ) -> Result<Option<SignalMessage>, webrtc::error::Error> {
        let key = self.key(room, peer_id);
        match signal.kind.as_str() {
            "offer" => {
                let pc = self.get_or_create(&key).await?;
                pc.set_remote_description(RTCSessionDescription::offer(signal.sdp.clone())?)
                    .await?;
                let answer = pc.create_answer(None).await?;
                pc.set_local_description(answer.clone()).await?;
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
        let Some((room, _source_peer)) = source_key.split_once(':') else {
            return Ok(());
        };
        let kind = source.kind().await;
        let mut codec = None;
        for ssrc in source.ssrcs().await {
            if let Some(value) = source.codec(ssrc).await {
                codec = Some(value);
                break;
            }
        }
        let Some(codec) = codec else { return Ok(()) };
        let Some(source_ssrc) = source.ssrcs().await.first().copied() else {
            return Ok(());
        };
        let destinations: Vec<(String, Arc<dyn PeerConnection>)> = self
            .connections
            .lock()
            .await
            .iter()
            .filter(|(key, _)| key.starts_with(&format!("{room}:")) && key.as_str() != source_key)
            .map(|(key, pc)| (key.clone(), pc.clone()))
            .collect();

        let mut outputs = Vec::new();
        for (destination_key, pc) in destinations {
            let track_id = source.track_id().await;
            let relay_key = format!("{source_key}->{destination_key}:{track_id}");
            if self.relays.lock().await.contains_key(&relay_key) {
                continue;
            }
            let output = Arc::new(TrackLocalStaticRTP::new(MediaStreamTrack::new(
                format!("relay-{room}"),
                format!("relay-{track_id}"),
                "server-relay".to_owned(),
                kind,
                vec![RTCRtpEncodingParameters {
                    rtp_coding_parameters: RTCRtpCodingParameters {
                        ssrc: Some(source_ssrc),
                        ..Default::default()
                    },
                    codec: codec.clone(),
                    ..Default::default()
                }],
            )));
            pc.add_track(output.clone() as Arc<dyn TrackLocal>).await?;
            self.relays.lock().await.insert(relay_key, output.clone());

            let offer = pc.create_offer(None).await?;
            pc.set_local_description(offer.clone()).await?;
            let destination_peer = destination_key.split(':').nth(1).unwrap_or_default();
            self.send_to_peer(
                &destination_key,
                SignalMessage {
                    kind: "offer".to_owned(),
                    room: room.to_owned(),
                    peer_id: destination_peer.to_owned(),
                    from: "server".to_owned(),
                    to: destination_peer.to_owned(),
                    sdp: offer.sdp,
                    candidate: None,
                },
            )
            .await;

            outputs.push(output);
        }

        tokio::spawn(async move {
            while let Some(event) = source.poll().await {
                if let TrackRemoteEvent::OnRtpPacket(packet) = event {
                    for output in &outputs {
                        let mut packet = packet.clone();
                        if let Some(ssrc) = output.ssrcs().await.first().copied() {
                            packet.header.ssrc = ssrc;
                        }
                        if output.write_rtp(packet).await.is_err() {
                            break;
                        }
                    }
                }
            }
        });
        let _ = codec;
        Ok(())
    }

    async fn send_to_peer(&self, key: &str, signal: SignalMessage) {
        if let Some(tx) = self.senders.lock().await.get(key) {
            if let Ok(payload) = serde_json::to_string(&signal) {
                let _ = tx.send(payload);
            }
        }
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

    async fn get(&self, key: &str) -> Result<Arc<dyn PeerConnection>, webrtc::error::Error> {
        self.connections.lock().await.get(key).cloned().ok_or_else(|| {
            webrtc::error::Error::from_std(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("peer connection not found for {key}"),
            ))
        })
    }

    fn key(&self, room: &str, peer: &str) -> String {
        format!("{room}:{peer}")
    }

    pub async fn remove_peer(&self, room: &str, peer: &str) {
        let key = self.key(room, peer);
        if let Some(pc) = self.connections.lock().await.remove(&key) {
            let _ = pc.close().await;
        }
        self.senders.lock().await.remove(&key);
        self.relays.lock().await.retain(|relay, _| !relay.contains(&key));
    }
}
