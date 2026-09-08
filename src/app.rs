use std::{
    collections::HashMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    extract::{Query, State, WebSocketUpgrade},
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::get,
    Json, Router,
};
use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::sync::{mpsc, Mutex};

use crate::signaling::{RoomQuery, SignalMessage};

#[derive(Clone, Debug, Default)]
pub struct AppState {
    rooms: Arc<Mutex<HashMap<String, Vec<Peer>>>>,
    webrtc: Arc<crate::webrtc::WebRtcAgent>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            rooms: Arc::new(Mutex::new(HashMap::new())),
            webrtc: Arc::new(crate::webrtc::WebRtcAgent::new()),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Peer {
    id: String,
    tx: mpsc::UnboundedSender<String>,
}

pub async fn start_server() {
    let state = AppState::new();

    let app = Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .route("/ws", get(websocket_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .unwrap();

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
    ws.on_upgrade(move |socket| handle_socket(socket, room_id, state))
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

    let existing_peers;
    {
        let mut rooms = state.rooms.lock().await;
        existing_peers = rooms
            .get(&room_id)
            .map(|peers| peers.iter().map(|peer| peer.id.clone()).collect::<Vec<_>>())
            .unwrap_or_default();
        rooms.entry(room_id.clone()).or_default().push(Peer {
            id: peer_id.clone(),
            tx: tx.clone(),
        });
    }

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
            let Ok(signal) = serde_json::from_str::<SignalMessage>(&text) else {
                continue;
            };

            let mut signal = signal;
            signal.room = room_id.clone();
            signal.peer_id = peer_id.clone();
            signal.from = peer_id.clone();

            if let Err(error) = state
                .webrtc
                .process_signal(&signal, &room_id, &peer_id)
                .await
            {
                eprintln!("webrtc error for peer {} in room {}: {}", peer_id, room_id, error);
            }

            let payload = match serde_json::to_string(&signal) {
                Ok(value) => value,
                Err(_) => continue,
            };

            let peers = {
                let rooms = state.rooms.lock().await;
                rooms.get(&room_id).cloned().unwrap_or_default()
            };

            let target_peer = signal.to.clone();
            for peer in peers {
                if peer.id == peer_id {
                    continue;
                }

                if !target_peer.is_empty() && peer.id != target_peer {
                    continue;
                }

                let _ = peer.tx.send(payload.clone());
            }
        }
    }

    send_task.abort();
    state.webrtc.remove_peer(&room_id, &peer_id).await;

    let mut rooms = state.rooms.lock().await;
    if let Some(peers) = rooms.get_mut(&room_id) {
        peers.retain(|peer| peer.id != peer_id);
        if peers.is_empty() {
            rooms.remove(&room_id);
        }
    }
}
