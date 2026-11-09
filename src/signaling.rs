use serde::{Deserialize, Serialize};

pub const MAX_ROOM_ID_LENGTH: usize = 64;
pub const MAX_WS_MESSAGE_SIZE: usize = 64 * 1024;

pub fn validate_room_id(room_id: &str) -> Result<(), String> {
    let trimmed = room_id.trim();
    if trimmed.is_empty() {
        return Err("Room ID cannot be empty.".to_owned());
    }
    if room_id != trimmed {
        return Err("Room ID cannot include leading or trailing whitespace.".to_owned());
    }
    if room_id.len() > MAX_ROOM_ID_LENGTH {
        return Err(format!(
            "Room ID must be {} characters or fewer.",
            MAX_ROOM_ID_LENGTH
        ));
    }
    if room_id
        .chars()
        .any(|ch| !ch.is_ascii_alphanumeric() && !matches!(ch, '-' | '_'))
    {
        return Err("Room ID may only contain letters, numbers, '-' and '_'".to_owned());
    }
    Ok(())
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ParticipantState {
    #[serde(default)]
    pub peer_id: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub camera_on: bool,
    #[serde(default)]
    pub screen_share: bool,
    #[serde(default)]
    pub admitted: bool,
    #[serde(default)]
    pub connected: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
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
    pub display_name: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub camera_on: bool,
    #[serde(default)]
    pub screen_share: bool,
    #[serde(default)]
    pub sdp: String,
    #[serde(default)]
    pub candidate: Option<CandidateMessage>,
    #[serde(default)]
    pub participants: Vec<ParticipantState>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct CandidateMessage {
    #[serde(default)]
    pub candidate: String,
    #[serde(default, rename = "sdpMid", alias = "sdp_mid")]
    pub sdp_mid: Option<String>,
    #[serde(default, rename = "sdpMLineIndex", alias = "sdp_mline_index")]
    pub sdp_mline_index: Option<u16>,
}

#[derive(Debug, Deserialize)]
pub struct RoomQuery {
    pub room: Option<String>,
    pub ticket: Option<String>,
    pub display_name: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{CandidateMessage, SignalMessage, validate_room_id};

    #[test]
    fn parses_webrtc_signals() {
        let raw = r#"{"type":"offer","room":"demo-room","peer_id":"peer-1","sdp":"v=0"}"#;
        let value: SignalMessage = serde_json::from_str(raw).unwrap();
        assert_eq!(value.kind, "offer");
        assert_eq!(value.room, "demo-room");
        assert_eq!(value.peer_id, "peer-1");
        assert_eq!(value.sdp, "v=0");
    }

    #[test]
    fn room_ids_follow_the_allowed_rules() {
        assert!(validate_room_id("demo-room").is_ok());
        assert!(validate_room_id("room_7").is_ok());
        assert!(validate_room_id("Alpha-42").is_ok());

        assert!(validate_room_id("").is_err());
        assert!(validate_room_id("demo room").is_err());
        assert!(validate_room_id("bad/room").is_err());
        assert!(validate_room_id("x".repeat(65).as_str()).is_err());
    }

    #[test]
    fn ice_candidate_metadata_uses_browser_field_names() {
        let candidate = CandidateMessage {
            candidate: "candidate:example".to_owned(),
            sdp_mid: Some("0".to_owned()),
            sdp_mline_index: Some(0),
        };

        let serialized = serde_json::to_value(&candidate).unwrap();
        assert_eq!(serialized["sdpMid"], "0");
        assert_eq!(serialized["sdpMLineIndex"], 0);
        assert!(serialized.get("sdp_mid").is_none());
        assert!(serialized.get("sdp_mline_index").is_none());
    }
}
