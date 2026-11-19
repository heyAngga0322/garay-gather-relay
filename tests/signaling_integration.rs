//! Signaling Path Integration Tests
//!
//! These tests exercise the full Axum/WebSocket signaling path with real WebSocket clients
//! against a running server, complementing the in-process WebRtcAgent unit tests.
//!
//! The architecture is SFU (Selective Forwarding Unit): each browser establishes a WebRTC
//! connection to the server. The server does NOT forward offers/answers between peers.
//! Instead, each peer negotiates with the server directly.

use futures_util::{SinkExt, StreamExt};
use metrics_exporter_prometheus::PrometheusHandle;
use serde_json::{Value, json};
use serial_test::serial;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::time::timeout;
use tokio_tungstenite::{connect_async, tungstenite::Message};

static METRICS_HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

const WS_PATH: &str = "/ws";

/// Get a free port for testing
fn get_test_addr() -> String {
    // Use port 0 to let OS assign a free port, then get the actual port
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    format!("127.0.0.1:{}", addr.port())
}

/// Test helper to start a test server on a random port
async fn start_test_server(addr: &str) -> tokio::task::JoinHandle<()> {
    let listener = TcpListener::bind(addr).await.unwrap();

    let handle = tokio::spawn(async move {
        // Install metrics recorder only once globally
        let metrics_handle = METRICS_HANDLE
            .get_or_init(|| {
                metrics_exporter_prometheus::PrometheusBuilder::new()
                    .install_recorder()
                    .expect("failed to install metrics recorder")
            })
            .clone();

        let app = garay::app::create_app(metrics_handle, 4, None).await;

        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                tokio::signal::ctrl_c().await.ok();
            })
            .await
            .unwrap();
    });

    // Wait for server to be ready
    tokio::time::sleep(Duration::from_millis(500)).await;

    handle
}

/// Create a meeting and get host ticket
async fn create_meeting(addr: &str, display_name: &str) -> (String, String) {
    let client = reqwest::Client::new();
    let resp = client
        .post(&format!("http://{}/api/meeting", addr))
        .json(&json!({ "display_name": display_name }))
        .send()
        .await
        .unwrap();

    assert!(resp.status().is_success());
    let body: Value = resp.json().await.unwrap();
    (
        body["room_id"].as_str().unwrap().to_string(),
        body["host_ticket"].as_str().unwrap().to_string(),
    )
}

/// Request a guest ticket
async fn request_guest_ticket(addr: &str, room_id: &str, display_name: &str) -> String {
    let client = reqwest::Client::new();
    let resp = client
        .post(&format!("http://{}/api/meeting/{}/ticket", addr, room_id))
        .json(&json!({ "display_name": display_name }))
        .send()
        .await
        .unwrap();

    assert!(resp.status().is_success());
    let body: Value = resp.json().await.unwrap();
    body["ticket"].as_str().unwrap().to_string()
}

/// Connect WebSocket with ticket
async fn connect_ws(
    addr: &str,
    room_id: &str,
    ticket: &str,
) -> (
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    Value,
) {
    let url = format!(
        "ws://{}{}?room={}&ticket={}",
        addr, WS_PATH, room_id, ticket
    );
    let (mut ws_stream, _resp) = connect_async(&url).await.unwrap();

    // Read welcome message
    let msg = timeout(Duration::from_secs(5), ws_stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let welcome: Value = serde_json::from_str(&msg.to_text().unwrap()).unwrap();

    assert_eq!(welcome["type"], "welcome");
    assert_eq!(welcome["room"], room_id);

    (ws_stream, welcome)
}

/// Send a signaling message
async fn send_signal(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    msg: Value,
) {
    ws.send(Message::Text(msg.to_string().into()))
        .await
        .unwrap();
}

/// Receive a signaling message with timeout
async fn recv_signal(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> Value {
    let msg = timeout(Duration::from_secs(5), ws.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let text = msg.to_text().unwrap();
    let value: Value = serde_json::from_str(text).unwrap();
    // eprintln!("RECV: {}", value["type"]);
    value
}

/// Receive signals until we get one matching the expected type, or timeout
async fn recv_signal_of_type(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    expected_type: &str,
    max_attempts: usize,
) -> Value {
    for attempt in 0..max_attempts {
        let msg = recv_signal(ws).await;
        eprintln!(
            "Attempt {}: got message type '{}', expected '{}'",
            attempt + 1,
            msg["type"],
            expected_type
        );
        if msg["type"] == expected_type {
            return msg;
        }
    }
    panic!(
        "Expected message type '{}' not received after {} attempts",
        expected_type, max_attempts
    );
}

/// Valid SDP offer for testing (with fingerprint)
const TEST_SDP_OFFER: &str = "v=0\r\no=- 123456 2 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\na=group:BUNDLE 0\r\na=msid-semantic: WMS\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\nc=IN IP4 0.0.0.0\r\na=rtcp:9 IN IP4 0.0.0.0\r\na=ice-ufrag:test\r\na=ice-pwd:test\r\na=fingerprint:sha-256 12:34:56:78:90:AB:CD:EF:12:34:56:78:90:AB:CD:EF:12:34:56:78:90:AB:CD:EF:12:34:56:78:90:AB:CD:EF\r\na=mid:0\r\na=sendrecv\r\na=rtpmap:96 VP8/90000\r\n";

#[tokio::test]
#[serial]
async fn test_full_signaling_flow_two_peers_sfu() {
    // Start server on random port
    let addr = get_test_addr();
    let server_handle = start_test_server(&addr).await;

    // Create meeting
    let (room_id, host_ticket) = create_meeting(&addr, "Host").await;
    println!("Created room: {}", room_id);

    // Request guest ticket
    let guest_ticket = request_guest_ticket(&addr, &room_id, "Guest").await;
    println!("Got guest ticket");

    // Connect both peers
    let (mut host_ws, host_welcome) = connect_ws(&addr, &room_id, &host_ticket).await;
    let (mut guest_ws, guest_welcome) = connect_ws(&addr, &room_id, &guest_ticket).await;

    let host_peer_id = host_welcome["peer_id"].as_str().unwrap();
    let guest_peer_id = guest_welcome["peer_id"].as_str().unwrap();

    println!("Host peer_id: {}", host_peer_id);
    println!("Guest peer_id: {}", guest_peer_id);

    // Host should receive peer-joined for guest (and possibly roster)
    let host_peer_joined = recv_signal_of_type(&mut host_ws, "peer-joined", 3).await;
    assert_eq!(host_peer_joined["peer_id"], guest_peer_id);

    // Guest's welcome already included the roster (host)
    // Consume any roster message for guest
    let _ = recv_signal(&mut guest_ws).await; // roster from broadcast

    // In SFU architecture, each peer sends offer TO THE SERVER (not to other peers)
    // Server responds with answer

    // Host sends offer to server
    let host_offer = json!({
        "type": "offer",
        "room": room_id,
        "to": "server",  // SFU: offer goes to server
        "sdp": TEST_SDP_OFFER
    });
    send_signal(&mut host_ws, host_offer).await;

    // Host receives answer from server
    let host_answer = recv_signal_of_type(&mut host_ws, "answer", 3).await;
    assert_eq!(host_answer["type"], "answer");
    assert_eq!(host_answer["from"], "server");
    assert_eq!(host_answer["to"], host_peer_id);

    // Guest sends offer to server
    let guest_offer = json!({
        "type": "offer",
        "room": room_id,
        "to": "server",
        "sdp": TEST_SDP_OFFER
    });
    send_signal(&mut guest_ws, guest_offer).await;

    // Guest receives answer from server
    let guest_answer = recv_signal_of_type(&mut guest_ws, "answer", 3).await;
    assert_eq!(guest_answer["type"], "answer");
    assert_eq!(guest_answer["from"], "server");
    assert_eq!(guest_answer["to"], guest_peer_id);

    // Exchange ICE candidates with server
    let host_candidate = json!({
        "type": "candidate",
        "room": room_id,
        "to": "server",
        "candidate": "candidate:1 1 UDP 2122260223 192.168.1.1 54400 typ host"
    });
    send_signal(&mut host_ws, host_candidate).await;

    // Server may echo candidate back or send its own - just verify no error
    let _ = recv_signal(&mut host_ws).await;

    let guest_candidate = json!({
        "type": "candidate",
        "room": room_id,
        "to": "server",
        "candidate": "candidate:2 1 UDP 2122260223 192.168.1.2 54401 typ host"
    });
    send_signal(&mut guest_ws, guest_candidate).await;

    let _ = recv_signal(&mut guest_ws).await;

    // Clean disconnect
    host_ws.close(None).await.unwrap();
    guest_ws.close(None).await.unwrap();

    server_handle.abort();
    println!("✅ Two-peer SFU signaling flow test passed");
}

#[tokio::test]
#[serial]
async fn test_server_overwrites_untrusted_fields() {
    let addr = get_test_addr();
    let server_handle = start_test_server(&addr).await;

    let (room_id, host_ticket) = create_meeting(&addr, "Host").await;
    let guest_ticket = request_guest_ticket(&addr, &room_id, "Guest").await;

    let (mut host_ws, host_welcome) = connect_ws(&addr, &room_id, &host_ticket).await;
    let (mut guest_ws, guest_welcome) = connect_ws(&addr, &room_id, &guest_ticket).await;

    let host_peer_id = host_welcome["peer_id"].as_str().unwrap();
    let _guest_peer_id = guest_welcome["peer_id"].as_str().unwrap();

    // Consume peer-joined + roster messages (host receives both for guest)
    recv_signal(&mut host_ws).await; // peer-joined
    recv_signal(&mut host_ws).await; // roster
    // Guest's welcome already included roster
    let _ = recv_signal(&mut guest_ws).await; // roster from broadcast

    // Host sends offer with MALICIOUS untrusted fields (SFU: offer to server)
    let malicious_offer = json!({
        "type": "offer",
        "room": "different-room",  // Should be overwritten
        "from": "fake-peer-id",    // Should be overwritten
        "to": "server",            // SFU: offer goes to server
        "sdp": TEST_SDP_OFFER
    });
    send_signal(&mut host_ws, malicious_offer).await;

    // Host receives answer from server (with corrected fields)
    let received = recv_signal_of_type(&mut host_ws, "answer", 3).await;
    assert_eq!(received["room"], room_id); // Server's room_id
    assert_eq!(received["from"], "server"); // Server's identity
    assert_eq!(received["to"], host_peer_id);

    host_ws.close(None).await.unwrap();
    guest_ws.close(None).await.unwrap();

    server_handle.abort();
    println!("✅ Untrusted field overwrite test passed");
}

#[tokio::test]
#[serial]
async fn test_room_capacity_enforcement() {
    let addr = get_test_addr();
    let server_handle = start_test_server(&addr).await;

    let (room_id, host_ticket) = create_meeting(&addr, "Host").await;

    // Fill room to capacity (4 peers including host)
    let mut tickets = vec![host_ticket];
    for i in 1..4 {
        let ticket = request_guest_ticket(&addr, &room_id, &format!("Guest{}", i)).await;
        tickets.push(ticket);
    }

    // Connect all 4 peers
    let mut connections = Vec::new();
    for (i, ticket) in tickets.iter().enumerate() {
        let (ws, welcome) = connect_ws(&addr, &room_id, ticket).await;
        let peer_id = welcome["peer_id"].as_str().unwrap().to_string();
        connections.push((ws, welcome));
        println!("Peer {} connected: {}", i, peer_id);
    }

    // Consume peer-joined + roster messages
    // Each new peer triggers: peer-joined for each existing peer + roster for all
    // We'll just drain messages for a short time
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(2) {
        let mut any = false;
        for (ws, _) in &mut connections {
            if let Ok(msg) = timeout(Duration::from_millis(50), recv_signal(ws)).await {
                any = true;
                eprintln!("Capacity test got message: {}", msg["type"]);
            }
        }
        if !any {
            break;
        }
    }

    // Try to add 5th peer - should fail at WebSocket upgrade
    let guest5_ticket = request_guest_ticket(&addr, &room_id, "Guest5").await;
    let ws_result = timeout(
        Duration::from_secs(2),
        connect_async(format!(
            "ws://{}{}?room={}&ticket={}",
            addr, WS_PATH, room_id, guest5_ticket
        )),
    )
    .await;

    // Should fail or be rejected
    assert!(ws_result.is_err() || ws_result.unwrap().is_err());

    // Clean up
    for (mut ws, _) in connections {
        ws.close(None).await.ok();
    }

    server_handle.abort();
    println!("✅ Room capacity enforcement test passed");
}

#[tokio::test]
#[serial]
async fn test_ice_config_endpoint() {
    let addr = get_test_addr();
    let server_handle = start_test_server(&addr).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(&format!("http://{}/api/ice-config", addr))
        .send()
        .await
        .unwrap();

    assert!(resp.status().is_success());
    let body: Value = resp.json().await.unwrap();
    assert!(body["iceServers"].is_array());

    server_handle.abort();
    println!("✅ ICE config endpoint test passed");
}

#[tokio::test]
#[serial]
async fn test_health_endpoints() {
    let addr = get_test_addr();
    let server_handle = start_test_server(&addr).await;

    let client = reqwest::Client::new();

    // Health
    let resp = client
        .get(&format!("http://{}/health", addr))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");
    assert_eq!(body["service"], "webrtc-signal-server");

    // Ready
    let resp = client
        .get(&format!("http://{}/ready", addr))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ready");

    // Live
    let resp = client
        .get(&format!("http://{}/live", addr))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "alive");

    server_handle.abort();
    println!("✅ Health endpoints test passed");
}

#[tokio::test]
#[serial]
async fn test_metrics_endpoint() {
    let addr = get_test_addr();
    let server_handle = start_test_server(&addr).await;

    let client = reqwest::Client::new();

    // Create a meeting to trigger metrics
    let _ = client
        .post(&format!("http://{}/api/meeting", addr))
        .json(&json!({ "display_name": "Test Host" }))
        .send()
        .await
        .unwrap();

    let resp = client
        .get(&format!("http://{}/metrics", addr))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());

    let body = resp.text().await.unwrap();
    eprintln!(
        "Metrics body (first 500 chars): {}",
        &body[..body.len().min(500)]
    );
    // Check for garay metrics
    assert!(body.contains("garay_") || body.contains("# HELP") || body.contains("# TYPE"));

    server_handle.abort();
    println!("✅ Metrics endpoint test passed");
}

#[tokio::test]
#[serial]
async fn test_reconnect_recovery() {
    let addr = get_test_addr();
    let server_handle = start_test_server(&addr).await;

    let (room_id, host_ticket) = create_meeting(&addr, "Host").await;
    let guest_ticket = request_guest_ticket(&addr, &room_id, "Guest").await;

    // Initial connection
    let (mut host_ws, host_welcome) = connect_ws(&addr, &room_id, &host_ticket).await;
    let (mut guest_ws, guest_welcome) = connect_ws(&addr, &room_id, &guest_ticket).await;

    let host_peer_id = host_welcome["peer_id"].as_str().unwrap();
    let _guest_peer_id = guest_welcome["peer_id"].as_str().unwrap();

    // Consume peer-joined + roster (host receives both for guest)
    recv_signal(&mut host_ws).await; // peer-joined
    recv_signal(&mut host_ws).await; // roster
    // Guest's welcome already included roster

    // Host disconnects
    host_ws.close(None).await.unwrap();

    // Guest receives peer-left (and possibly roster)
    let peer_left = recv_signal_of_type(&mut guest_ws, "peer-left", 3).await;
    assert_eq!(peer_left["peer_id"], host_peer_id);

    // Host reconnects with same ticket (should fail - ticket consumed)
    // For reconnect, need new ticket or use host ticket again
    let (mut host_ws2, host_welcome2) = connect_ws(&addr, &room_id, &host_ticket).await;
    let new_host_peer_id = host_welcome2["peer_id"].as_str().unwrap();

    // Should get new peer ID
    assert_ne!(new_host_peer_id, host_peer_id);

    // Guest receives new peer-joined (and possibly roster)
    let peer_joined = recv_signal_of_type(&mut guest_ws, "peer-joined", 3).await;
    assert_eq!(peer_joined["peer_id"], new_host_peer_id);

    host_ws2.close(None).await.unwrap();
    guest_ws.close(None).await.unwrap();

    server_handle.abort();
    println!("✅ Reconnect recovery test passed");
}
