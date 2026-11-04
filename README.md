# WebRTC Axum Signaling Demo

A modular Rust WebRTC media relay built with Axum, Tokio, and the `webrtc` crate. The application serves a browser-based camera, microphone, and screen-sharing demo, manages room membership over WebSockets, and forwards encoded RTP through server-side peer connections.

Each browser establishes a WebRTC connection with the Rust server. The server copies incoming RTP packets from each participant to the other participants in the room.

## Features

- Axum HTTP server with WebSocket support
- Room-scoped peer management
- Server-assigned peer identities
- Server-side `offer`, `answer`, and ICE candidate handling
- RTP media forwarding through the Rust backend
- Vue 3 browser UI loaded from the unpkg CDN
- Browser camera and microphone access
- Local and remote video elements
- Microphone overlay toggle with on/off icons
- Camera toggle with on/off state
- Screen sharing with automatic camera restoration
- `/health` endpoint
- Server-side peer connection storage keyed by `room_id:peer_id`

## Requirements

- Rust toolchain with Cargo
- A modern browser supporting:
  - `navigator.mediaDevices.getUserMedia`
  - `RTCPeerConnection`
  - WebSocket
- Internet access to load Vue 3 from the unpkg CDN
- Camera and microphone permissions

## Run

```sh
cargo run
```

The server listens on:

```text
http://127.0.0.1:3000
```

The room peer limit defaults to 2. Set `MAX_PEERS_PER_ROOM` to configure the
admission limit; keep it at 2 until multi-participant media negotiation has
been validated and capacity has been measured on the deployment hardware.

Open the application in two browser tabs. Grant camera and microphone permissions in both tabs, then use the `Start Call` button if negotiation does not start automatically.

The default room is `demo-room`. The WebSocket endpoint also accepts an explicit room:

```text
ws://127.0.0.1:3000/ws?room=my-room
```

## Room and WebSocket Limits

The server enforces a narrow room ID format and room membership cap:

- Room IDs must be 1-64 characters long.
- Allowed characters are ASCII letters, numbers, `-`, and `_`.
- Whitespace and separators like `/` are rejected.
- Each room can admit at most 2 peers at a time.
- WebSocket text messages larger than 64 KiB are rejected before signal parsing.

These limits are checked before the connection is upgraded and before signaling is processed, so invalid rooms and oversized payloads are rejected cleanly.

## Verify the Server

Health check:

```sh
curl -s http://127.0.0.1:3000/health
```

Expected response:

```json
{"service":"webrtc-signal-server","status":"ok"}
```

## Development Commands

```sh
cargo check
cargo test
cargo fmt --check
```

The signaling schema test is located in `src/signaling.rs`.

## Architecture

```text
Browser A                  Rust Axum Server                  Browser B
    |                              |                              |
    |--- WebSocket connect ------->|                              |
    |<------ welcome --------------|                              |
    |                              |<------ WebSocket connect ----|
    |<------ peer-joined ----------|------ peer-joined ---------->|
    |                              |                              |
    |---------- offer ------------>|                              |
    |<---------- answer -----------|                              |
    |------- ICE candidate -------->|                              |
    |<========== WebRTC media =====|                              |
    |                              |========== WebRTC media =====>|
    |                              |<========== WebRTC media =====|
    |<========== WebRTC media =====|                              |
```

### Rust modules

- `src/main.rs`: Tokio application entrypoint.
- `src/app.rs`: Axum routes, shared state, room membership, WebSocket handling, forwarding, and cleanup.
- `src/signaling.rs`: Serde models for signaling messages and ICE candidates.
- `src/ui.rs`: Embedded HTML, CSS, Vue 3 UI, and browser-side WebRTC logic.
- `src/webrtc.rs`: Server-side `webrtc` crate peer connection management.

## Signaling Messages

Messages use the following structure:

```json
{
  "type": "offer",
  "room": "demo-room",
  "peer_id": "peer-123",
  "from": "peer-123",
  "to": "peer-456",
  "sdp": "v=0...",
  "candidate": null
}
```

Supported message types include:

- `welcome`: assigns the canonical peer ID to a WebSocket client.
- `peer-joined`: informs room peers about membership changes.
- `offer`: carries an SDP offer.
- `answer`: carries an SDP answer.
- `candidate`: carries an ICE candidate.

The server overwrites incoming `room`, `peer_id`, and `from` values with the values associated with the WebSocket connection. Browser SDP and ICE messages terminate at the server. Server-generated SDP and ICE messages are sent back to the browser that owns the corresponding server-side peer connection.

## Browser Call Flow

1. The browser connects to `/ws?room=demo-room`.
2. The server sends a `welcome` message with a server-generated peer ID.
3. The browser creates an offer for its server-side peer connection.
4. The server returns an answer and receives the browser's RTP tracks.
5. The server creates relay tracks on the other participants' peer connections.
6. The server renegotiates those connections and forwards encoded audio/video RTP.

The microphone icon is displayed over the top-right of the local video. The camera control is displayed next to `Start Call`. Both controls update their icon, visual state, tooltip, and accessibility state when toggled.

## Current Limitations

- The default browser UI uses a fixed `demo-room` value.
- The server always binds to port `3000`.
- There is no authentication or authorization.
- There is no room capacity limit or persistent room storage.
- The relay currently keeps one server-side peer connection per browser and forwards encoded RTP without decoding it.
- Track cleanup and renegotiation are optimized for the demo's small rooms; a production SFU would need stronger lifecycle and backpressure handling.
- The demo uses Google's public STUN server. TURN configuration may be needed on restrictive networks.
- End-to-end calls require two browser clients and appropriate camera/microphone permissions.

## Acknowledgements

This project builds on the work of the WebRTC.rs team and the Vue.js team. Their open-source contributions and tooling made the signaling and browser UI layers practical and approachable.

## License

No license has been specified for this project yet.
