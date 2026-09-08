use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SignalMessage {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub room: String,
    #[serde(default)]
    pub peer_id: String,
    #[serde(default)]
    pub from: String,
    #[serde(default)]
    pub to: String,
    #[serde(default)]
    pub sdp: String,
    #[serde(default)]
    pub candidate: Option<CandidateMessage>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct CandidateMessage {
    #[serde(default)]
    pub candidate: String,
    #[serde(default, alias = "sdpMid")]
    pub sdp_mid: Option<String>,
    #[serde(default, alias = "sdpMLineIndex")]
    pub sdp_mline_index: Option<u16>,
}

#[derive(Debug, Deserialize)]
pub struct RoomQuery {
    pub room: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::SignalMessage;

    #[test]
    fn parses_webrtc_signals() {
        let raw = r#"{"type":"offer","room":"demo-room","peer_id":"peer-1","sdp":"v=0"}"#;
        let value: SignalMessage = serde_json::from_str(raw).unwrap();
        assert_eq!(value.kind, "offer");
        assert_eq!(value.room, "demo-room");
        assert_eq!(value.peer_id, "peer-1");
        assert_eq!(value.sdp, "v=0");
    }
}
