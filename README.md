# WebRTC Axum Signaling Demo

A modular Rust WebRTC signaling server built with Axum, Tokio, and the `webrtc` crate. The application serves a browser-based camera and microphone demo, manages room membership over WebSockets, and forwards targeted SDP and ICE signaling messages between peers.

The browser establishes the media connection directly with another browser using the native `RTCPeerConnection` API. The Rust server handles signaling and maintains a server-side `webrtc` peer connection for each room/peer lifecycle.

## Features

- Axum HTTP server with WebSocket support
- Room-scoped peer management
- Server-assigned peer identities
- Targeted `offer`, `answer`, and ICE candidate forwarding
- Browser camera and microphone access
- Local and remote video elements
- Microphone overlay toggle with on/off icons
- Camera toggle with on/off state
- `/health` endpoint
- Server-side peer connection storage keyed by `room_id:peer_id`

## Requirements

- Rust toolchain with Cargo
- A modern browser supporting:
  - `navigator.mediaDevices.getUserMedia`
  - `RTCPeerConnection`
  - WebSocket
- Camera and microphone permissions

## Run

```sh
cargo run
```

The server listens on:

```text
http://127.0.0.1:3000
```

Open the application in two browser tabs. Grant camera and microphone permissions in both tabs, then use the `Start Call` button if negotiation does not start automatically.

The default room is `demo-room`. The WebSocket endpoint also accepts an explicit room:

```text
ws://127.0.0.1:3000/ws?room=my-room
```

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
    |---------- offer ------------>|---------- offer ------------>|
    |<---------- answer -----------|<---------- answer ------------|
    |------- ICE candidate -------->|------ ICE candidate -------->|
    |                              |                              |
    |<========== direct WebRTC media connection ==================>|
```

### Rust modules

- `src/main.rs`: Tokio application entrypoint.
- `src/app.rs`: Axum routes, shared state, room membership, WebSocket handling, forwarding, and cleanup.
- `src/signaling.rs`: Serde models for signaling messages and ICE candidates.
- `src/ui.rs`: Embedded HTML, CSS, and browser-side WebRTC logic.
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

The server overwrites incoming `room`, `peer_id`, and `from` values with the values associated with the WebSocket connection. The `to` field is used to forward SDP and ICE messages only to the intended peer.

## Browser Call Flow

1. The browser connects to `/ws?room=demo-room`.
2. The server sends a `welcome` message with a server-generated peer ID.
3. Both peers receive a `peer-joined` notification.
4. The peer with the lexicographically smaller ID creates the offer.
5. The other peer applies the offer and returns an answer.
6. Both peers exchange ICE candidates.
7. Local audio/video tracks are sent through the browser-to-browser WebRTC connection.

The microphone icon is displayed over the top-right of the local video. The camera control is displayed next to `Start Call`. Both controls update their icon, visual state, tooltip, and accessibility state when toggled.

## Current Limitations

- The default browser UI uses a fixed `demo-room` value.
- The server always binds to port `3000`.
- There is no authentication or authorization.
- There is no room capacity limit or persistent room storage.
- Browser media does not pass through the Rust server.
- The server-side `webrtc` peer connection is currently used for signaling-side SDP/ICE processing and lifecycle management, not media forwarding.
- The demo uses Google's public STUN server. TURN configuration may be needed on restrictive networks.
- End-to-end calls require two browser clients and appropriate camera/microphone permissions.

## License

No license has been specified for this project yet.
