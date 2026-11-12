use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};

use axum::extract::ws::{Message, WebSocket};
use axum::{
    Json, Router,
    extract::{Path, Query, State, WebSocketUpgrade},
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post},
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::{Mutex, mpsc};

use crate::signaling::{MAX_WS_MESSAGE_SIZE, RoomQuery, SignalMessage, validate_room_id};

pub const MAX_PEERS_PER_ROOM: usize = 4;
pub const MAX_DISPLAY_NAME_LENGTH: usize = 64;
pub const MAX_SIGNALING_STATE_BYTES: usize = 4 * 1024;
pub const MAX_OUTBOUND_QUEUE_MESSAGES: usize = 32;
#[allow(dead_code)]
pub const MAX_SIGNALING_MESSAGES_PER_SECOND: usize = 10;

pub fn validate_display_name(display_name: &str) -> Result<(), String> {
    let trimmed = display_name.trim();
    if trimmed.is_empty() {
        return Err("Display name cannot be empty.".to_owned());
    }
    if trimmed.len() > MAX_DISPLAY_NAME_LENGTH {
        return Err(format!(
            "Display name must be {} characters or fewer.",
            MAX_DISPLAY_NAME_LENGTH
        ));
    }
    Ok(())
}

pub fn validate_signal_state(value: &str) -> Result<(), String> {
    let size = value.as_bytes().len();
    if size > MAX_SIGNALING_STATE_BYTES {
        return Err(format!(
            "Signal state exceeds the limit of {} bytes.",
            MAX_SIGNALING_STATE_BYTES
        ));
    }
    Ok(())
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct RateLimiter {
    max_events: usize,
    window: Duration,
    timestamps: Arc<tokio::sync::Mutex<VecDeque<Instant>>>,
}

#[allow(dead_code)]
impl RateLimiter {
    pub fn new(max_events: usize, window: Duration) -> Self {
        Self {
            max_events,
            window,
            timestamps: Arc::new(tokio::sync::Mutex::new(VecDeque::new())),
        }
    }

    pub fn check(&self) -> bool {
        let mut timestamps = self.timestamps.blocking_lock();
        let now = Instant::now();
        while let Some(front) = timestamps.front() {
            if now.duration_since(*front) <= self.window {
                break;
            }
            timestamps.pop_front();
        }
        if timestamps.len() >= self.max_events {
            return false;
        }
        timestamps.push_back(now);
        true
    }
}

#[derive(Clone, Debug)]
pub struct AppState {
    rooms: Arc<Mutex<HashMap<String, Vec<Peer>>>>,
    meetings: Arc<Mutex<HashMap<String, MeetingState>>>,
    webrtc: Arc<crate::webrtc::WebRtcAgent>,
    allowed_origins: Arc<Vec<String>>,
    max_peers_per_room: usize,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct MeetingTicket {
    #[allow(dead_code)]
    pub room_id: String,
    pub role: String,
    #[allow(dead_code)]
    pub nonce: String,
    pub display_name: String,
    pub expires_at_ms: u128,
    pub consumed: bool,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct MeetingState {
    #[allow(dead_code)]
    pub room_id: String,
    pub owner_name: String,
    pub host_ticket: String,
    pub guest_tickets: HashMap<String, MeetingTicket>,
    pub members: Vec<String>,
    pub participants: HashMap<String, crate::signaling::ParticipantState>,
    pub lock: bool,
    pub lobby: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateMeetingRequest {
    pub display_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateMeetingResponse {
    pub room_id: String,
    pub invite_url: String,
    pub host_ticket: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GuestTicketRequest {
    pub display_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GuestTicketResponse {
    pub room_id: String,
    pub ticket: String,
    pub invite_url: String,
}

#[allow(dead_code)]
impl AppState {
    pub fn new() -> Self {
        Self::with_room_capacity(MAX_PEERS_PER_ROOM)
    }

    pub fn with_room_capacity(max_peers_per_room: usize) -> Self {
        Self::with_policy(max_peers_per_room, Vec::new())
    }

    pub fn with_policy(max_peers_per_room: usize, allowed_origins: Vec<String>) -> Self {
        Self {
            rooms: Arc::new(Mutex::new(HashMap::new())),
            meetings: Arc::new(Mutex::new(HashMap::new())),
            webrtc: Arc::new(crate::webrtc::WebRtcAgent::new()),
            allowed_origins: Arc::new(allowed_origins),
            max_peers_per_room: max_peers_per_room.max(1),
        }
    }

    pub async fn create_meeting(
        &self,
        display_name: &str,
    ) -> Result<CreateMeetingResponse, String> {
        let trimmed = display_name.trim();
        validate_display_name(trimmed)?;

        let room_id = self.next_room_id().await;
        let host_ticket = self.make_ticket(&room_id, "host").await;
        let mut rooms = self.rooms.lock().await;
        rooms.entry(room_id.clone()).or_default();
        drop(rooms);

        let mut meetings = self.meetings.lock().await;
        meetings.insert(
            room_id.clone(),
            MeetingState {
                room_id: room_id.clone(),
                owner_name: trimmed.to_owned(),
                host_ticket: host_ticket.clone(),
                guest_tickets: HashMap::new(),
                members: vec![trimmed.to_owned()],
                participants: HashMap::new(),
                lock: false,
                lobby: false,
            },
        );

        Ok(CreateMeetingResponse {
            room_id: room_id.clone(),
            invite_url: format!("http://127.0.0.1:3000/?room={room_id}&ticket={host_ticket}"),
            host_ticket,
        })
    }

    pub async fn request_guest_ticket(
        &self,
        room_id: &str,
        display_name: &str,
    ) -> Result<GuestTicketResponse, String> {
        let trimmed = display_name.trim();
        validate_display_name(trimmed)?;
        validate_room_id(room_id)?;

        let mut meetings = self.meetings.lock().await;
        let meeting = meetings
            .get_mut(room_id)
            .ok_or_else(|| format!("Unknown meeting: {room_id}"))?;
        if meeting.lock {
            return Err(format!("Meeting {room_id} is locked."));
        }

        let ticket = self.make_ticket(room_id, "guest").await;
        let expires_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            + 3_600_000;

        meeting.guest_tickets.insert(
            ticket.clone(),
            MeetingTicket {
                room_id: room_id.to_owned(),
                role: "guest".to_owned(),
                nonce: ticket.split(':').last().unwrap_or_default().to_owned(),
                display_name: trimmed.to_owned(),
                expires_at_ms,
                consumed: false,
            },
        );

        Ok(GuestTicketResponse {
            room_id: room_id.to_owned(),
            ticket: ticket.clone(),
            invite_url: format!("http://127.0.0.1:3000/?room={room_id}&ticket={ticket}"),
        })
    }

    pub async fn consume_ticket(&self, room_id: &str, ticket: &str) -> Result<String, String> {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();

        let mut meetings = self.meetings.lock().await;
        let meeting = meetings
            .get_mut(room_id)
            .ok_or_else(|| format!("Unknown meeting: {room_id}"))?;

        if ticket == meeting.host_ticket {
            return Ok("host".to_string());
        }

        let guest_ticket = meeting
            .guest_tickets
            .get_mut(ticket)
            .ok_or_else(|| format!("Unknown or invalid join ticket for room {room_id}"))?;

        if guest_ticket.consumed {
            return Err(format!(
                "Join ticket for room {room_id} has already been used."
            ));
        }
        if guest_ticket.expires_at_ms <= now_ms {
            return Err(format!("Join ticket for room {room_id} has expired."));
        }

        guest_ticket.consumed = true;
        Ok(guest_ticket.role.clone())
    }

    pub async fn validate_ticket(&self, room_id: &str, ticket: &str) -> bool {
        self.consume_ticket(room_id, ticket).await.is_ok()
    }

    pub async fn resolve_ticket_access(
        &self,
        room_id: &str,
        ticket: &str,
        display_name: Option<&str>,
    ) -> Result<TicketAccess, String> {
        resolve_ticket_access(self, room_id, ticket, display_name).await
    }

    pub async fn register_participant(
        &self,
        room_id: &str,
        peer_id: &str,
        access: &TicketAccess,
    ) -> Result<crate::signaling::ParticipantState, String> {
        let mut meetings = self.meetings.lock().await;
        let meeting = meetings
            .get_mut(room_id)
            .ok_or_else(|| format!("Unknown meeting: {room_id}"))?;

        validate_display_name(&access.display_name)?;

        let admitted = access.role == "host" || !meeting.lobby;
        let participant = meeting
            .participants
            .entry(peer_id.to_owned())
            .or_insert_with(|| crate::signaling::ParticipantState {
                peer_id: peer_id.to_owned(),
                display_name: access.display_name.clone(),
                role: access.role.clone(),
                muted: false,
                camera_on: true,
                screen_share: false,
                admitted,
                connected: true,
            });

        participant.peer_id = peer_id.to_owned();
        participant.display_name = access.display_name.clone();
        participant.role = access.role.clone();
        participant.admitted = admitted;
        participant.connected = true;

        if admitted
            && !meeting
                .members
                .iter()
                .any(|name| name == &participant.display_name)
        {
            meeting.members.push(participant.display_name.clone());
        }

        Ok(participant.clone())
    }

    pub async fn snapshot_roster(&self, room_id: &str) -> Vec<crate::signaling::ParticipantState> {
        let meetings = self.meetings.lock().await;
        let Some(meeting) = meetings.get(room_id) else {
            return Vec::new();
        };
        meeting
            .participants
            .values()
            .filter(|participant| participant.connected)
            .cloned()
            .collect()
    }

    pub async fn broadcast_roster_snapshot(&self, room_id: &str) {
        let snapshot = self.snapshot_roster(room_id).await;
        let payload = serde_json::to_string(&SignalMessage {
            kind: "roster".to_string(),
            room: room_id.to_owned(),
            peer_id: String::new(),
            from: String::new(),
            to: String::new(),
            display_name: String::new(),
            role: String::new(),
            muted: false,
            camera_on: false,
            screen_share: false,
            sdp: String::new(),
            candidate: None,
            participants: snapshot,
        })
        .unwrap_or_default();

        let peers = {
            let rooms = self.rooms.lock().await;
            rooms
                .get(room_id)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .collect::<Vec<_>>()
        };

        for peer in peers {
            if peer.tx.try_send(payload.clone()).is_err() {
                eprintln!("Dropping roster update for slow client in room {room_id}");
            }
        }
    }

    pub async fn set_meeting_lobby(
        &self,
        room_id: &str,
        host_ticket: &str,
        enabled: bool,
    ) -> Result<(), String> {
        let mut meetings = self.meetings.lock().await;
        let meeting = meetings
            .get_mut(room_id)
            .ok_or_else(|| format!("Unknown meeting: {room_id}"))?;
        if host_ticket != meeting.host_ticket {
            return Err("Only the meeting host can change the lobby state.".to_owned());
        }

        meeting.lobby = enabled;
        for participant in meeting.participants.values_mut() {
            if participant.role == "guest" {
                participant.admitted = !enabled;
                participant.connected = participant.admitted;
            }
        }
        Ok(())
    }

    pub async fn admit_guest(
        &self,
        room_id: &str,
        host_ticket: &str,
        peer_id: &str,
    ) -> Result<(), String> {
        let mut meetings = self.meetings.lock().await;
        let meeting = meetings
            .get_mut(room_id)
            .ok_or_else(|| format!("Unknown meeting: {room_id}"))?;
        if host_ticket != meeting.host_ticket {
            return Err("Only the host can admit guests.".to_owned());
        }
        let participant = meeting
            .participants
            .get_mut(peer_id)
            .ok_or_else(|| format!("Participant {peer_id} is not in the meeting."))?;
        participant.admitted = true;
        participant.connected = true;
        Ok(())
    }

    pub async fn remove_participant(
        &self,
        room_id: &str,
        actor_ticket: &str,
        peer_id: &str,
    ) -> Result<(), String> {
        let mut meetings = self.meetings.lock().await;
        let meeting = meetings
            .get_mut(room_id)
            .ok_or_else(|| format!("Unknown meeting: {room_id}"))?;
        let actor_allowed =
            actor_ticket == meeting.host_ticket || meeting.participants.get(actor_ticket).is_some();
        if !actor_allowed {
            return Err("Only the host or the participant can remove a member.".to_owned());
        }
        meeting.participants.remove(peer_id);
        Ok(())
    }

    pub async fn mute_participant(
        &self,
        room_id: &str,
        host_ticket: &str,
        peer_id: &str,
        muted: bool,
    ) -> Result<(), String> {
        let mut meetings = self.meetings.lock().await;
        let meeting = meetings
            .get_mut(room_id)
            .ok_or_else(|| format!("Unknown meeting: {room_id}"))?;
        if host_ticket != meeting.host_ticket {
            return Err("Only the host can mute participants.".to_owned());
        }
        let participant = meeting
            .participants
            .get_mut(peer_id)
            .ok_or_else(|| format!("Participant {peer_id} is not in the meeting."))?;
        participant.muted = muted;
        Ok(())
    }

    async fn next_room_id(&self) -> String {
        const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut bytes = [0u8; 12];
        getrandom::fill(&mut bytes).expect("failed to generate room id");
        let mut room = String::with_capacity(12);
        for value in bytes {
            let idx = (value as usize) % CHARSET.len();
            room.push(CHARSET[idx] as char);
        }
        room
    }

    async fn make_ticket(&self, room_id: &str, role: &str) -> String {
        let mut bytes = [0u8; 18];
        getrandom::fill(&mut bytes).expect("failed to generate ticket nonce");
        let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
        let mut nonce = String::with_capacity(18);
        for value in bytes {
            let idx = (value as usize) % alphabet.len();
            nonce.push(alphabet[idx] as char);
        }
        format!("{role}:{room_id}:{nonce}")
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug)]
pub struct Peer {
    id: String,
    tx: mpsc::Sender<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum RoomAdmissionError {
    InvalidRoomId(String),
    RoomFull { room: String, limit: usize },
}

pub fn validate_message_size(message: &str) -> Result<(), String> {
    let size = message.as_bytes().len();
    if size > MAX_WS_MESSAGE_SIZE {
        return Err(format!(
            "WebSocket message size {} exceeds the limit of {} bytes.",
            size, MAX_WS_MESSAGE_SIZE
        ));
    }
    Ok(())
}

pub async fn try_register_peer(
    state: &AppState,
    room_id: &str,
    peer_id: &str,
    tx: mpsc::Sender<String>,
) -> Result<(), RoomAdmissionError> {
    validate_room_id(room_id).map_err(RoomAdmissionError::InvalidRoomId)?;

    let mut rooms = state.rooms.lock().await;
    let room_members = rooms.entry(room_id.to_string()).or_default();
    if room_members.len() >= state.max_peers_per_room {
        return Err(RoomAdmissionError::RoomFull {
            room: room_id.to_string(),
            limit: state.max_peers_per_room,
        });
    }

    room_members.push(Peer {
        id: peer_id.to_string(),
        tx,
    });
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TicketAccess {
    pub room_id: String,
    pub role: String,
    pub display_name: String,
}

pub async fn resolve_ticket_access(
    state: &AppState,
    room_id: &str,
    ticket: &str,
    display_name: Option<&str>,
) -> Result<TicketAccess, String> {
    let trimmed_room = room_id.trim();
    validate_room_id(trimmed_room)?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();

    let mut meetings = state.meetings.lock().await;
    let meeting = meetings
        .get_mut(trimmed_room)
        .ok_or_else(|| format!("Unknown meeting: {trimmed_room}"))?;

    if ticket == meeting.host_ticket {
        return Ok(TicketAccess {
            room_id: trimmed_room.to_owned(),
            role: "host".to_owned(),
            display_name: meeting.owner_name.clone(),
        });
    }

    let guest_ticket = meeting
        .guest_tickets
        .get_mut(ticket)
        .ok_or_else(|| format!("Unknown or invalid join ticket for room {trimmed_room}"))?;

    if guest_ticket.consumed {
        return Err(format!(
            "Join ticket for room {trimmed_room} has already been used."
        ));
    }
    if guest_ticket.expires_at_ms <= now_ms {
        return Err(format!("Join ticket for room {trimmed_room} has expired."));
    }
    if meeting.lobby {
        return Err("This meeting is in the lobby. A host must admit the guest first.".to_owned());
    }

    guest_ticket.consumed = true;
    let resolved_name = display_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(guest_ticket.display_name.trim());
    if resolved_name.is_empty() {
        return Err("Display name cannot be empty.".to_owned());
    }

    Ok(TicketAccess {
        room_id: trimmed_room.to_owned(),
        role: guest_ticket.role.clone(),
        display_name: resolved_name.to_owned(),
    })
}

pub fn origin_is_allowed<S>(origin: &str, allowed_origins: &[S]) -> bool
where
    S: AsRef<str>,
{
    let normalized_origin = origin.trim();
    if normalized_origin.is_empty() {
        return false;
    }
    allowed_origins.iter().any(|candidate| {
        candidate
            .as_ref()
            .trim()
            .trim_end_matches('/')
            .eq_ignore_ascii_case(normalized_origin.trim_end_matches('/'))
    })
}

pub async fn start_server() {
    let room_capacity = std::env::var("MAX_PEERS_PER_ROOM")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(MAX_PEERS_PER_ROOM);
    let allowed_origins: Vec<String> = std::env::var("WEBRTC_ALLOWED_ORIGINS")
        .or_else(|_| std::env::var("ALLOWED_ORIGINS"))
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let app_env = std::env::var("APP_ENV").unwrap_or_else(|_| "development".to_owned());
    if app_env == "production" && allowed_origins.is_empty() {
        panic!("WEBRTC_ALLOWED_ORIGINS must be configured in production mode.");
    }
    let state = AppState::with_policy(room_capacity, allowed_origins);

    let app = Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .route("/api/meeting", post(create_meeting_handler))
        .route(
            "/api/meeting/{room}/ticket",
            post(request_guest_ticket_handler),
        )
        .route("/ws", get(websocket_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();

    println!("WebRTC signaling server is running on http://0.0.0.0:3000");
    axum::serve(listener, app).await.unwrap();
}

async fn health() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({
            "status": "ok",
            "service": "webrtc-signal-server"
        })),
    )
}

async fn index() -> Html<&'static str> {
    Html(crate::ui::INDEX_HTML)
}

async fn create_meeting_handler(
    State(state): State<AppState>,
    Json(payload): Json<CreateMeetingRequest>,
) -> impl IntoResponse {
    match state.create_meeting(&payload.display_name).await {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))).into_response(),
    }
}

async fn request_guest_ticket_handler(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Json(payload): Json<GuestTicketRequest>,
) -> impl IntoResponse {
    match state
        .request_guest_ticket(&room_id, &payload.display_name)
        .await
    {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))).into_response(),
    }
}

async fn websocket_handler(
    Query(query): Query<RoomQuery>,
    headers: axum::http::HeaderMap,
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let room_id = query.room.unwrap_or_else(|| "demo-room".to_string());
    if let Err(message) = validate_room_id(&room_id) {
        return (
            StatusCode::BAD_REQUEST,
            format!("Invalid room ID: {message}"),
        )
            .into_response();
    }

    let origin = headers
        .get("origin")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if !state.allowed_origins.is_empty() {
        match origin {
            Some(value) if origin_is_allowed(value, &state.allowed_origins) => {}
            _ => {
                return (
                    StatusCode::FORBIDDEN,
                    "Origin not allowed for this signaling endpoint.".to_string(),
                )
                    .into_response();
            }
        }
    }

    let access = if let Some(ticket) = query.ticket.clone() {
        match state
            .resolve_ticket_access(&room_id, &ticket, query.display_name.as_deref())
            .await
        {
            Ok(access) => Some(access),
            Err(message) => {
                return (
                    StatusCode::UNAUTHORIZED,
                    format!("Ticket rejected: {message}"),
                )
                    .into_response();
            }
        }
    } else {
        query
            .display_name
            .filter(|name| !name.trim().is_empty())
            .map(|display_name| TicketAccess {
                room_id: room_id.clone(),
                role: "guest".to_string(),
                display_name: display_name.trim().to_string(),
            })
    };

    ws.max_message_size(MAX_WS_MESSAGE_SIZE)
        .on_upgrade(move |socket| handle_socket(socket, room_id, access, state))
}

#[cfg(test)]
mod tests {
    use super::{
        AppState, MAX_OUTBOUND_QUEUE_MESSAGES, MAX_PEERS_PER_ROOM, MAX_SIGNALING_STATE_BYTES,
        RateLimiter, RoomAdmissionError, origin_is_allowed, resolve_ticket_access,
        try_register_peer, validate_display_name, validate_message_size, validate_signal_state,
    };
    use crate::signaling::MAX_WS_MESSAGE_SIZE;

    #[test]
    fn oversized_websocket_messages_are_rejected() {
        assert!(validate_message_size("small").is_ok());
        let oversized = "x".repeat(MAX_WS_MESSAGE_SIZE + 1);
        assert!(validate_message_size(&oversized).is_err());
    }

    #[tokio::test]
    async fn rooms_reject_peers_after_capacity_is_reached() {
        let state = AppState::new();
        let room_id = "capacity-room";

        for peer_number in 1..=MAX_PEERS_PER_ROOM {
            let (tx, _rx) = tokio::sync::mpsc::channel(MAX_OUTBOUND_QUEUE_MESSAGES);
            assert!(
                try_register_peer(&state, room_id, &format!("peer-{peer_number}"), tx)
                    .await
                    .is_ok()
            );
        }

        let (tx, _rx) = tokio::sync::mpsc::channel(MAX_OUTBOUND_QUEUE_MESSAGES);
        match try_register_peer(
            &state,
            room_id,
            &format!("peer-{}", MAX_PEERS_PER_ROOM + 1),
            tx,
        )
        .await
        {
            Err(RoomAdmissionError::RoomFull { limit, .. }) => {
                assert_eq!(limit, MAX_PEERS_PER_ROOM);
            }
            other => panic!("expected room-full error, got: {other:?}"),
        }

        assert_eq!(
            state
                .rooms
                .lock()
                .await
                .get(room_id)
                .map(Vec::len)
                .unwrap_or_default(),
            MAX_PEERS_PER_ROOM
        );
    }

    #[tokio::test]
    async fn configured_room_capacity_allows_three_peers() {
        let state = AppState::with_room_capacity(3);

        for peer_id in ["peer-1", "peer-2", "peer-3"] {
            let (tx, _rx) = tokio::sync::mpsc::channel(MAX_OUTBOUND_QUEUE_MESSAGES);
            assert!(
                try_register_peer(&state, "three-peer-room", peer_id, tx)
                    .await
                    .is_ok()
            );
        }

        let (tx, _rx) = tokio::sync::mpsc::channel(MAX_OUTBOUND_QUEUE_MESSAGES);
        match try_register_peer(&state, "three-peer-room", "peer-4", tx).await {
            Err(RoomAdmissionError::RoomFull { limit, .. }) => assert_eq!(limit, 3),
            other => panic!("expected room-full error at configured capacity, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn rejected_peers_do_not_leave_stale_room_state() {
        let state = AppState::new();
        let room_id = "cleanup-room";

        for peer_number in 1..=MAX_PEERS_PER_ROOM {
            let (tx, _rx) = tokio::sync::mpsc::channel(MAX_OUTBOUND_QUEUE_MESSAGES);
            assert!(
                try_register_peer(&state, room_id, &format!("peer-{peer_number}"), tx)
                    .await
                    .is_ok()
            );
        }

        let rejected_peer = format!("peer-{}", MAX_PEERS_PER_ROOM + 1);
        let (tx, _rx) = tokio::sync::mpsc::channel(MAX_OUTBOUND_QUEUE_MESSAGES);
        assert!(
            try_register_peer(&state, room_id, &rejected_peer, tx)
                .await
                .is_err()
        );

        let room = state
            .rooms
            .lock()
            .await
            .get(room_id)
            .cloned()
            .unwrap_or_default();
        assert_eq!(room.len(), MAX_PEERS_PER_ROOM);
        assert!(room.iter().all(|peer| peer.id != rejected_peer));
    }

    #[tokio::test]
    async fn remaining_peers_receive_a_departure_notification() {
        use axum::{Router, routing::get};
        use futures_util::StreamExt;
        use serde_json::Value;
        use tokio_tungstenite::{connect_async, tungstenite::Message};

        let state = AppState::new();
        let app = Router::new()
            .route("/ws", get(super::websocket_handler))
            .with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let (mut first, _) = connect_async(format!("ws://{address}/ws?room=leave-test"))
            .await
            .unwrap();
        let first_welcome: Value = match first.next().await.unwrap().unwrap() {
            Message::Text(payload) => serde_json::from_str(payload.as_ref()).unwrap(),
            message => panic!("expected welcome text message, got {message:?}"),
        };

        let (mut second, _) = connect_async(format!("ws://{address}/ws?room=leave-test"))
            .await
            .unwrap();
        let _second_welcome = second.next().await.unwrap().unwrap();
        let joined: Value = match first.next().await.unwrap().unwrap() {
            Message::Text(payload) => {
                let value: Value = serde_json::from_str(payload.as_ref()).unwrap();
                match value["type"].as_str() {
                    Some("peer-joined") => value,
                    Some("roster") => {
                        let next = first.next().await.unwrap().unwrap();
                        match next {
                            Message::Text(payload) => {
                                serde_json::from_str(payload.as_ref()).unwrap()
                            }
                            message => panic!("expected peer-joined text message, got {message:?}"),
                        }
                    }
                    other => panic!("expected peer-joined or roster message, got {other:?}"),
                }
            }
            message => panic!("expected peer-joined text message, got {message:?}"),
        };
        assert_eq!(joined["type"], "peer-joined");

        second.close(None).await.unwrap();
        let left: Value = loop {
            match first.next().await.unwrap().unwrap() {
                Message::Text(payload) => {
                    let value: Value = serde_json::from_str(payload.as_ref()).unwrap();
                    if value["type"].as_str() == Some("peer-left") {
                        break value;
                    }
                }
                message => panic!("expected peer-left text message, got {message:?}"),
            }
        };
        assert_eq!(left["type"], "peer-left");
        assert_ne!(left["peer_id"], first_welcome["peer_id"]);

        server.abort();
    }

    #[tokio::test]
    async fn ticket_auth_and_origin_checks_follow_server_policy() {
        let state = AppState::new();
        let meeting = state.create_meeting("Host User").await.unwrap();
        let guest_ticket = state
            .request_guest_ticket(&meeting.room_id, "Guest User")
            .await
            .unwrap()
            .ticket;

        let host_access =
            resolve_ticket_access(&state, &meeting.room_id, &meeting.host_ticket, None)
                .await
                .unwrap();
        assert_eq!(host_access.role, "host");

        let guest_access = resolve_ticket_access(&state, &meeting.room_id, &guest_ticket, None)
            .await
            .unwrap();
        assert_eq!(guest_access.role, "guest");
        assert!(
            resolve_ticket_access(&state, &meeting.room_id, &guest_ticket, None)
                .await
                .is_err()
        );

        let allowed = origin_is_allowed("https://example.com", &["https://example.com"]);
        assert!(allowed);
        assert!(!origin_is_allowed(
            "https://evil.test",
            &["https://example.com"]
        ));
    }

    #[test]
    fn participant_name_and_state_payloads_are_bounded() {
        let long_name = "x".repeat(256);
        assert!(validate_display_name(&long_name).is_err());
        assert!(validate_display_name("   ").is_err());
        assert!(validate_display_name("Alina").is_ok());

        let oversized_state = "x".repeat(MAX_SIGNALING_STATE_BYTES + 1);
        assert!(validate_signal_state(&oversized_state).is_err());
        assert!(validate_signal_state("online").is_ok());
    }

    #[test]
    fn rate_limiter_blocks_bursts() {
        let limiter = RateLimiter::new(2, std::time::Duration::from_secs(1));
        assert!(limiter.check());
        assert!(limiter.check());
        assert!(!limiter.check());
    }
}

async fn handle_socket(
    socket: WebSocket,
    room_id: String,
    access: Option<TicketAccess>,
    state: AppState,
) {
    let mut peer_id = String::with_capacity(18);
    peer_id.push_str("peer-");
    let alphabet = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut bytes = [0u8; 12];
    getrandom::fill(&mut bytes).expect("failed to generate peer id");
    for value in bytes {
        let idx = (value as usize) % alphabet.len();
        peer_id.push(alphabet[idx] as char);
    }

    let display_name = access
        .as_ref()
        .map(|item| item.display_name.clone())
        .unwrap_or_else(|| "Guest".to_string());
    let role = access
        .as_ref()
        .map(|item| item.role.clone())
        .unwrap_or_else(|| "guest".to_string());

    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = mpsc::channel::<String>(MAX_OUTBOUND_QUEUE_MESSAGES);
    let (webrtc_tx, mut webrtc_rx) = mpsc::unbounded_channel::<String>();

    let existing_peers = {
        let rooms = state.rooms.lock().await;
        rooms
            .get(&room_id)
            .map(|peers| peers.iter().map(|peer| peer.id.clone()).collect::<Vec<_>>())
            .unwrap_or_default()
    };

    if let Err(error) = try_register_peer(&state, &room_id, &peer_id, tx.clone()).await {
        eprintln!("Room admission rejected for room {}: {error:?}", room_id);
        drop(sender);
        return;
    }

    if let Some(access) = access.as_ref() {
        let _ = state
            .register_participant(&room_id, &peer_id, access)
            .await
            .map_err(|error| eprintln!("participant registration failed: {error}"));
    }

    state
        .webrtc
        .register_peer(&room_id, &peer_id, webrtc_tx)
        .await;

    let socket_tx = tx.clone();
    let forward_webrtc = tokio::spawn(async move {
        while let Some(payload) = webrtc_rx.recv().await {
            if socket_tx.send(payload).await.is_err() {
                break;
            }
        }
    });

    let roster = state.snapshot_roster(&room_id).await;
    let welcome = SignalMessage {
        kind: "welcome".to_string(),
        room: room_id.clone(),
        peer_id: peer_id.clone(),
        from: String::new(),
        to: peer_id.clone(),
        display_name: display_name.clone(),
        role: role.clone(),
        muted: false,
        camera_on: true,
        screen_share: false,
        sdp: String::new(),
        candidate: None,
        participants: roster.clone(),
    };
    if let Ok(payload) = serde_json::to_string(&welcome) {
        let _ = tx.send(payload).await;
    }

    let joined_notification = SignalMessage {
        kind: "peer-joined".to_string(),
        room: room_id.clone(),
        peer_id: peer_id.clone(),
        from: peer_id.clone(),
        to: String::new(),
        display_name: display_name.clone(),
        role: role.clone(),
        muted: false,
        camera_on: true,
        screen_share: false,
        sdp: String::new(),
        candidate: None,
        participants: roster.clone(),
    };

    let joined_payload = serde_json::to_string(&joined_notification).unwrap();
    let peers = {
        let rooms = state.rooms.lock().await;
        rooms.get(&room_id).cloned().unwrap_or_default()
    };

    for peer in peers {
        if peer.id != peer_id {
            if peer.tx.send(joined_payload.clone()).await.is_err() {
                eprintln!("Dropping peer-joined update for slow client in room {room_id}");
            }
        }
    }

    for existing_peer_id in existing_peers {
        let notification = SignalMessage {
            kind: "peer-joined".to_string(),
            room: room_id.clone(),
            peer_id: existing_peer_id.clone(),
            from: existing_peer_id,
            to: peer_id.clone(),
            display_name: display_name.clone(),
            role: role.clone(),
            muted: false,
            camera_on: true,
            screen_share: false,
            sdp: String::new(),
            candidate: None,
            participants: roster.clone(),
        };
        if let Ok(payload) = serde_json::to_string(&notification) {
            let _ = tx.send(payload).await;
        }
    }

    state.broadcast_roster_snapshot(&room_id).await;

    let send_task = tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            if sender.send(Message::Text(message.into())).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(message)) = receiver.next().await {
        if let Message::Text(text) = message {
            if let Err(error) = validate_message_size(&text) {
                eprintln!(
                    "WebSocket message rejected for peer {} in room {}: {error}",
                    peer_id, room_id
                );
                break;
            }

            let Ok(signal) = serde_json::from_str::<SignalMessage>(&text) else {
                continue;
            };

            if !signal.display_name.trim().is_empty() {
                if let Err(error) = validate_display_name(&signal.display_name) {
                    eprintln!(
                        "Invalid display name for peer {} in room {}: {error}",
                        peer_id, room_id
                    );
                    break;
                }
                if let Err(error) = validate_signal_state(&signal.display_name) {
                    eprintln!(
                        "Display name payload too large for peer {} in room {}: {error}",
                        peer_id, room_id
                    );
                    break;
                }
            }

            let mut signal = signal;
            signal.room = room_id.clone();
            signal.peer_id = peer_id.clone();
            signal.from = peer_id.clone();

            if !signal.display_name.trim().is_empty() {
                let mut meetings = state.meetings.lock().await;
                if let Some(meeting) = meetings.get_mut(&room_id) {
                    if let Some(participant) = meeting.participants.get_mut(&peer_id) {
                        participant.display_name = signal.display_name.trim().to_string();
                    }
                }
            }

            if signal.kind == "state" || signal.kind == "participant-state" {
                if let Err(error) = validate_signal_state(&signal.display_name) {
                    eprintln!(
                        "State payload for peer {} in room {} rejected: {error}",
                        peer_id, room_id
                    );
                    break;
                }
                let mut meetings = state.meetings.lock().await;
                if let Some(meeting) = meetings.get_mut(&room_id) {
                    if let Some(participant) = meeting.participants.get_mut(&peer_id) {
                        participant.muted = signal.muted;
                        participant.camera_on = signal.camera_on;
                        participant.screen_share = signal.screen_share;
                        participant.display_name = signal.display_name.trim().to_string();
                    }
                }
                drop(meetings);
                state.broadcast_roster_snapshot(&room_id).await;
            }

            match state
                .webrtc
                .process_signal(&signal, &room_id, &peer_id)
                .await
            {
                Ok(Some(response)) => {
                    if let Ok(payload) = serde_json::to_string(&response) {
                        let _ = tx.send(payload).await;
                    }
                    if response.kind == "answer"
                        && let Err(error) = state
                            .webrtc
                            .flush_queued_negotiation(&room_id, &peer_id)
                            .await
                    {
                        eprintln!(
                            "queued WebRTC negotiation failed for peer {} in room {}: {}",
                            peer_id, room_id, error
                        );
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    eprintln!(
                        "webrtc error for peer {} in room {}: {}",
                        peer_id, room_id, error
                    );
                }
            }
        }
    }

    send_task.abort();
    forward_webrtc.abort();
    state.webrtc.remove_peer(&room_id, &peer_id).await;

    let remaining_peers = {
        let mut rooms = state.rooms.lock().await;
        let remaining_peers = if let Some(peers) = rooms.get_mut(&room_id) {
            peers.retain(|peer| peer.id != peer_id);
            peers.iter().map(|peer| peer.tx.clone()).collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        if rooms.get(&room_id).is_some_and(Vec::is_empty) {
            rooms.remove(&room_id);
        }
        remaining_peers
    };

    let left_notification = SignalMessage {
        kind: "peer-left".to_string(),
        room: room_id.clone(),
        peer_id: peer_id.clone(),
        from: peer_id,
        to: String::new(),
        display_name: display_name,
        role: role,
        muted: false,
        camera_on: true,
        screen_share: false,
        sdp: String::new(),
        candidate: None,
        participants: Vec::new(),
    };
    if let Ok(payload) = serde_json::to_string(&left_notification) {
        for peer in remaining_peers {
            if peer.try_send(payload.clone()).is_err() {
                eprintln!("Dropping peer-left update for slow client in room {room_id}");
            }
        }
    }
}
