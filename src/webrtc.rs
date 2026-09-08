use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use tokio::sync::Mutex;
use webrtc::peer_connection::{
    PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler, RTCConfigurationBuilder,
    RTCIceCandidateInit, RTCIceServer, RTCSessionDescription,
};

#[derive(Default)]
pub struct WebRtcAgent {
    connections: Arc<Mutex<HashMap<String, Arc<dyn PeerConnection>>>>,
}

impl std::fmt::Debug for WebRtcAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let count = self.connections.blocking_lock().len();
        f.debug_struct("WebRtcAgent")
            .field("connections", &count)
            .finish()
    }
}

#[derive(Clone)]
struct NoopPeerConnectionHandler;

#[async_trait]
impl PeerConnectionEventHandler for NoopPeerConnectionHandler {
    async fn on_ice_candidate(&self, _event: webrtc::peer_connection::RTCPeerConnectionIceEvent) {}
}

impl WebRtcAgent {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn process_signal(
        &self,
        signal: &crate::signaling::SignalMessage,
        room_id: &str,
        peer_id: &str,
    ) -> Result<Option<crate::signaling::SignalMessage>, webrtc::error::Error> {
        let room_peer_key = self.connection_key(room_id, peer_id);

        match signal.kind.as_str() {
            "offer" => {
                let pc = self.get_or_create_peer_connection(&room_peer_key).await?;
                let remote = RTCSessionDescription::offer(signal.sdp.clone())?;
                pc.set_remote_description(remote).await?;

                let answer = pc.create_answer(None).await?;
                pc.set_local_description(answer.clone()).await?;

                Ok(Some(crate::signaling::SignalMessage {
                    kind: "answer".to_string(),
                    room: signal.room.clone(),
                    peer_id: peer_id.to_string(),
                    from: peer_id.to_string(),
                    to: signal.from.clone(),
                    sdp: answer.sdp,
                    candidate: None,
                }))
            }
            "answer" => {
                let pc = self.get_peer_connection(&room_peer_key).await?;
                let remote = RTCSessionDescription::answer(signal.sdp.clone())?;
                pc.set_remote_description(remote).await?;
                Ok(None)
            }
            "candidate" => {
                if let Some(candidate) = &signal.candidate {
                    let pc = self.get_peer_connection(&room_peer_key).await?;
                    let ice_candidate = RTCIceCandidateInit {
                        candidate: candidate.candidate.clone(),
                        sdp_mid: candidate.sdp_mid.clone(),
                        sdp_mline_index: candidate.sdp_mline_index,
                        username_fragment: None,
                        url: None,
                    };
                    pc.add_ice_candidate(ice_candidate).await?;
                }
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    fn connection_key(&self, room_id: &str, peer_id: &str) -> String {
        format!("{room_id}:{peer_id}")
    }

    async fn get_or_create_peer_connection(
        &self,
        peer_key: &str,
    ) -> Result<Arc<dyn PeerConnection>, webrtc::error::Error> {
        {
            let connections = self.connections.lock().await;
            if let Some(connection) = connections.get(peer_key) {
                return Ok(connection.clone());
            }
        }

        let config = RTCConfigurationBuilder::default()
            .with_ice_servers(vec![RTCIceServer {
                urls: vec!["stun:stun.l.google.com:19302".to_owned()],
                ..Default::default()
            }])
            .build();

        let pc = PeerConnectionBuilder::new()
            .with_configuration(config)
            .with_handler(Arc::new(NoopPeerConnectionHandler))
            .with_udp_addrs(vec!["0.0.0.0:0"])
            .build()
            .await?;
        let pc = Arc::new(pc);

        let mut connections = self.connections.lock().await;
        connections.insert(peer_key.to_string(), pc.clone());
        Ok(pc)
    }

    async fn get_peer_connection(
        &self,
        peer_key: &str,
    ) -> Result<Arc<dyn PeerConnection>, webrtc::error::Error> {
        let connections = self.connections.lock().await;
        connections
            .get(peer_key)
            .cloned()
            .ok_or_else(|| {
                webrtc::error::Error::from_std(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("peer connection not found for {peer_key}"),
                ))
            })
    }

    pub async fn remove_peer(&self, room_id: &str, peer_id: &str) {
        let mut connections = self.connections.lock().await;
        connections.remove(&self.connection_key(room_id, peer_id));
    }
}
