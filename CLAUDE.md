# CLAUDE.md

## Project Overview

`webrtc-axum` is a Rust WebRTC signaling demo. It serves an embedded browser UI over HTTP and relays WebRTC signaling messages over room-scoped WebSockets. The browser uses the native `RTCPeerConnection` API for the actual browser-to-browser media connection.

The server also creates a server-side `webrtc` crate `PeerConnection` for each connected room/peer pair. This currently provides signaling-side SDP/ICE processing and connection lifecycle management; browser media is not routed through the Rust server.

## Technology

- Rust edition 2024
- Tokio async runtime
- Axum 0.8 with WebSocket support
- `webrtc` 0.20.5
- Serde and Serde JSON for signaling payloads
- `futures-util` for WebSocket stream/sink handling

## Commands

Run from the repository root:

```sh
cargo check
cargo test
cargo run
```

The server listens on `0.0.0.0:3000`.

Health check:

```sh
curl -s http://127.0.0.1:3000/health
```

Expected response:

```json
{"service":"webrtc-signal-server","status":"ok"}
```

If a previous development process owns port 3000, identify or stop it before restarting. Avoid changing the port unless the task specifically requires configurable binding.

## Source Layout

- `src/main.rs`: Tokio entrypoint; delegates startup to `app::start_server()`.
- `src/app.rs`: Axum routes, shared application state, room membership, WebSocket lifecycle, targeted message forwarding, and cleanup.
- `src/signaling.rs`: Serde models for signaling messages, ICE candidates, and the `room` query parameter. Contains the signaling parsing test.
- `src/ui.rs`: Embedded HTML/CSS/JavaScript served at `/`. Contains the browser camera/microphone UI and native `RTCPeerConnection` negotiation.
- `src/webrtc.rs`: Server-side `webrtc` crate integration. Stores peer connections by `room_id:peer_id`, applies incoming SDP/ICE, and removes connections when sockets close.

## HTTP and WebSocket API

Routes:

- `GET /`: serves the embedded `INDEX_HTML` from `src/ui.rs`.
- `GET /health`: returns service status JSON.
- `GET /ws?room=<room-id>`: upgrades to a WebSocket. If `room` is omitted, the room defaults to `demo-room`.

`SignalMessage` is serialized with `type` as the JSON field for its Rust `kind` field. Common fields are:

- `type`: `welcome`, `peer-joined`, `offer`, `answer`, or `candidate`.
- `room`: server-normalized room ID.
- `peer_id`: server-assigned sender ID.
- `from`: server-normalized sender ID.
- `to`: target peer ID; an empty value means room broadcast for notifications.
- `sdp`: SDP text for offers and answers.
- `candidate`: optional ICE candidate object.

ICE JSON accepts browser field names `sdpMid` and `sdpMLineIndex` through Serde aliases.

The server always overwrites incoming `room`, `peer_id`, and `from` values with the values associated with the WebSocket. Do not trust client-provided identity fields for routing.

## Browser Negotiation Flow

1. A browser connects to `/ws?room=demo-room`.
2. The server sends `welcome` with the canonical peer ID.
3. The server sends `peer-joined` notifications to existing and newly connected peers.
4. The browser with the lexicographically smaller peer ID initiates the offer.
5. The other browser applies the offer, creates an answer, and sends it to the offerer.
6. Both browsers exchange ICE candidates targeted with `to`.
7. Local camera and microphone tracks are added to the browser `RTCPeerConnection`; remote tracks are displayed in the remote video element.

The microphone toggle is an icon overlay on the local video. The camera toggle remains in the action row. Both controls update their icon, color, tooltip, and `aria-pressed` state from the corresponding `MediaStreamTrack.enabled` value.

## Implementation Guidelines

- Preserve the existing module boundaries. Put routing and room lifecycle changes in `app.rs`, wire-format changes in `signaling.rs`, browser behavior in `ui.rs`, and server-side WebRTC changes in `webrtc.rs`.
- Match the installed `webrtc` 0.20.5 API. Inspect dependency sources when an API is unclear instead of assuming examples from another crate version.
- Keep signaling targeted with `to` whenever a message belongs to one peer. Do not broadcast SDP or ICE to every room member.
- Keep server identity authoritative. Browser-generated IDs must not be used for routing.
- When changing `SignalMessage`, update both Rust serialization/deserialization and the JavaScript payloads together.
- Run `cargo fmt --check`, `cargo check`, and `cargo test` after Rust changes. At minimum, run `cargo check` after changes limited to the embedded UI.
- Do not commit build artifacts from `target/`.

## Known Limitations

- The demo UI uses the fixed room `demo-room`; there is no room picker.
- The server binds only to port 3000 and has no configuration layer.
- There is no authentication, authorization, room capacity limit, or origin validation.
- The Rust-side `PeerConnection` is not yet a media forwarding server. Browser media is exchanged directly through browser WebRTC.
- End-to-end negotiation requires two browser tabs/windows, camera and microphone permissions, and a browser environment that supports `getUserMedia` and `RTCPeerConnection`.
- The current ICE configuration relies on Google's public STUN server and may need TURN support for restrictive networks.
