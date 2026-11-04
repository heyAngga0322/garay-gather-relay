use std::{
    collections::HashMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::extract::ws::{Message, WebSocket};
use axum::{
    Json, Router,
    extract::{Query, State, WebSocketUpgrade},
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::sync::{Mutex, mpsc};

use crate::signaling::{MAX_WS_MESSAGE_SIZE, RoomQuery, SignalMessage, validate_room_id};

pub const MAX_PEERS_PER_ROOM: usize = 2;

#[derive(Clone, Debug)]
pub struct AppState {
    rooms: Arc<Mutex<HashMap<String, Vec<Peer>>>>,
    webrtc: Arc<crate::webrtc::WebRtcAgent>,
    max_peers_per_room: usize,
}

impl AppState {
    pub fn new() -> Self {
        Self::with_room_capacity(MAX_PEERS_PER_ROOM)
    }

    pub fn with_room_capacity(max_peers_per_room: usize) -> Self {
        Self {
            rooms: Arc::new(Mutex::new(HashMap::new())),
            webrtc: Arc::new(crate::webrtc::WebRtcAgent::new()),
            max_peers_per_room: max_peers_per_room.max(1),
        }
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
    tx: mpsc::UnboundedSender<String>,
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
    tx: mpsc::UnboundedSender<String>,
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

pub async fn start_server() {
    let room_capacity = std::env::var("MAX_PEERS_PER_ROOM")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(MAX_PEERS_PER_ROOM);
    let state = AppState::with_room_capacity(room_capacity);

    let app = Router::new()
        .route("/", get(index))
        .route("/health", get(health))
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

async fn websocket_handler(
    Query(query): Query<RoomQuery>,
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

    ws.max_message_size(MAX_WS_MESSAGE_SIZE)
        .on_upgrade(move |socket| handle_socket(socket, room_id, state))
}

#[cfg(test)]
mod tests {
    use super::{
        AppState, MAX_PEERS_PER_ROOM, RoomAdmissionError, try_register_peer, validate_message_size,
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

        let (tx_first, _rx_first) = tokio::sync::mpsc::unbounded_channel();
        let (tx_second, _rx_second) = tokio::sync::mpsc::unbounded_channel();
        let (tx_third, _rx_third) = tokio::sync::mpsc::unbounded_channel();

        assert!(
            try_register_peer(&state, room_id, "peer-1", tx_first)
                .await
                .is_ok()
        );
        assert!(
            try_register_peer(&state, room_id, "peer-2", tx_second)
                .await
                .is_ok()
        );

        match try_register_peer(&state, room_id, "peer-3", tx_third).await {
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
            let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
            assert!(
                try_register_peer(&state, "three-peer-room", peer_id, tx)
                    .await
                    .is_ok()
            );
        }

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        match try_register_peer(&state, "three-peer-room", "peer-4", tx).await {
            Err(RoomAdmissionError::RoomFull { limit, .. }) => assert_eq!(limit, 3),
            other => panic!("expected room-full error at configured capacity, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn rejected_peers_do_not_leave_stale_room_state() {
        let state = AppState::new();
        let room_id = "cleanup-room";

        let (tx_first, _rx_first) = tokio::sync::mpsc::unbounded_channel();
        let (tx_second, _rx_second) = tokio::sync::mpsc::unbounded_channel();
        let (tx_third, _rx_third) = tokio::sync::mpsc::unbounded_channel();

        assert!(
            try_register_peer(&state, room_id, "peer-1", tx_first)
                .await
                .is_ok()
        );
        assert!(
            try_register_peer(&state, room_id, "peer-2", tx_second)
                .await
                .is_ok()
        );
        assert!(
            try_register_peer(&state, room_id, "peer-3", tx_third)
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
        assert!(room.iter().all(|peer| peer.id != "peer-3"));
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
            Message::Text(payload) => serde_json::from_str(payload.as_ref()).unwrap(),
            message => panic!("expected peer-joined text message, got {message:?}"),
        };
        assert_eq!(joined["type"], "peer-joined");

        second.close(None).await.unwrap();
        let left: Value = match first.next().await.unwrap().unwrap() {
            Message::Text(payload) => serde_json::from_str(payload.as_ref()).unwrap(),
            message => panic!("expected peer-left text message, got {message:?}"),
        };
        assert_eq!(left["type"], "peer-left");
        assert_ne!(left["peer_id"], first_welcome["peer_id"]);

        server.abort();
    }
}

async fn handle_socket(socket: WebSocket, room_id: String, state: AppState) {
    let peer_id = format!(
        "peer-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );

    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

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

    state
        .webrtc
        .register_peer(&room_id, &peer_id, tx.clone())
        .await;

    let welcome = SignalMessage {
        kind: "welcome".to_string(),
        room: room_id.clone(),
        peer_id: peer_id.clone(),
        from: String::new(),
        to: peer_id.clone(),
        sdp: String::new(),
        candidate: None,
    };
    let _ = tx.send(serde_json::to_string(&welcome).unwrap());

    let joined_notification = SignalMessage {
        kind: "peer-joined".to_string(),
        room: room_id.clone(),
        peer_id: peer_id.clone(),
        from: peer_id.clone(),
        to: String::new(),
        sdp: String::new(),
        candidate: None,
    };

    let joined_payload = serde_json::to_string(&joined_notification).unwrap();
    let peers = {
        let rooms = state.rooms.lock().await;
        rooms.get(&room_id).cloned().unwrap_or_default()
    };

    for peer in peers {
        if peer.id != peer_id {
            let _ = peer.tx.send(joined_payload.clone());
        }
    }

    for existing_peer_id in existing_peers {
        let notification = SignalMessage {
            kind: "peer-joined".to_string(),
            room: room_id.clone(),
            peer_id: existing_peer_id.clone(),
            from: existing_peer_id,
            to: peer_id.clone(),
            sdp: String::new(),
            candidate: None,
        };
        if let Ok(payload) = serde_json::to_string(&notification) {
            let _ = tx.send(payload);
        }
    }

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

            let mut signal = signal;
            signal.room = room_id.clone();
            signal.peer_id = peer_id.clone();
            signal.from = peer_id.clone();

            match state
                .webrtc
                .process_signal(&signal, &room_id, &peer_id)
                .await
            {
                Ok(Some(response)) => {
                    if let Ok(payload) = serde_json::to_string(&response) {
                        let _ = tx.send(payload);
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
        sdp: String::new(),
        candidate: None,
    };
    if let Ok(payload) = serde_json::to_string(&left_notification) {
        for peer in remaining_peers {
            let _ = peer.send(payload.clone());
        }
    }
}
