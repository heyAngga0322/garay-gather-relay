# WebRTC Axum Media Relay

A small-group meeting prototype built with Rust, Axum, Tokio, and the `webrtc` crate. It serves an embedded browser UI, manages room membership and signaling over WebSockets, and relays encoded RTP through server-side WebRTC peer connections. It is not yet a production-ready meeting service.

Each browser establishes a WebRTC connection to the Rust server. The server forwards incoming RTP to the other participants in the room; media is not sent directly between browsers.

## Features

- Axum HTTP server with `/`, `/health`, and room-scoped `/ws` endpoints
- Server-assigned peer identities and in-memory room membership
- Server-side SDP offer/answer handling, ICE exchange, and queued renegotiation
- RTP forwarding between participants through server-side peer connections
- Late-join replay of existing publications
- Embedded Vue 3 browser UI loaded from the unpkg CDN
- Browser camera, microphone, and screen-sharing controls
- Local preview and multiple remote video tiles
- Per-room peer limit, configurable with `MAX_PEERS_PER_ROOM` (default: 2)
- Maximum inbound WebSocket message size of 64 KiB
- `/health` endpoint

## Requirements

- Rust toolchain with Cargo
- A modern browser supporting:
  - `navigator.mediaDevices.getUserMedia`
  - `navigator.mediaDevices.getDisplayMedia` for screen sharing
  - `RTCPeerConnection`
  - WebSocket
- Internet access to load Vue 3 from the unpkg CDN and reach the configured STUN server
- Camera and microphone permissions

## Run

```sh
cargo run
```

The server binds to `0.0.0.0:3000`; open the app locally at:

```text
http://127.0.0.1:3000
```

The room peer limit defaults to 2. It can be changed for local testing, for example:

```sh
MAX_PEERS_PER_ROOM=3 cargo run
```

The current UI uses the fixed room `demo-room`. Open it in two browser tabs and grant camera and microphone permissions. The browser begins negotiation after the server assigns its peer ID; use `Start Call` if you need to retry local media setup. Screen sharing requires a browser that supports display capture.

For a different room, connect to the WebSocket endpoint directly with a valid room ID:

```text
ws://127.0.0.1:3000/ws?room=my-room
```

If the `room` query parameter is omitted, the server uses `demo-room`.

## Room and WebSocket Limits

The server enforces room ID, room capacity, and inbound WebSocket message limits:

- Room IDs must be 1-64 characters long.
- Allowed characters are ASCII letters, numbers, `-`, and `_`.
- Whitespace and separators like `/` are rejected.
- Each room admits at most `MAX_PEERS_PER_ROOM` peers; the default is 2.
- Inbound WebSocket messages larger than 64 KiB are rejected before signal parsing.

Room IDs are validated before the WebSocket upgrade. Capacity is enforced during room admission, and message size is enforced by Axum on the upgraded WebSocket.

## Verify the Server

Health check:

```sh
curl -s http://127.0.0.1:3000/health
```

Expected response:

```json
{"status":"ok","service":"webrtc-signal-server"}
```

## Development Commands

```sh
cargo check
cargo test
cargo fmt --check
```

The automated Rust tests cover room and message limits, signaling serialization, WebSocket membership events, offer/answer handling, renegotiation, RTP relay, late-join replay, and relay cleanup. The resource baselines are opt-in ignored tests; see `PROGRESS.md` for their methodology and recorded results.

## Architecture

```text
Browser A                    Rust Axum / WebRTC relay                    Browser B
   |                                  |                                      |
   |---------- WebSocket ------------>|<------------- WebSocket ------------|
   |<--- welcome / peer events ------>|------ welcome / peer events ------->|
   |                                  |                                      |
   |------ SDP offer / ICE ---------->|<--------- SDP offer / ICE ----------|
   |<----- SDP answer / ICE ----------|---------- SDP answer / ICE -------->|
   |                                  |                                      |
   |---------- RTP media ------------>|------------ RTP media ------------->|
   |<--------- relayed RTP -----------|<----------- relayed RTP -------------|
```

Each browser negotiates with its own server-side peer connection. The server routes signaling and forwards encoded RTP between room participants; it does not decode or transcode media.

### Rust modules

- `src/main.rs`: Tokio application entrypoint.
- `src/app.rs`: Axum routes, shared state, room admission, WebSocket handling, membership notifications, and cleanup.
- `src/signaling.rs`: Serde models for signaling messages and ICE candidates.
- `src/ui.rs`: Embedded HTML, CSS, Vue 3 UI, and browser-side WebRTC logic.
- `src/webrtc.rs`: Server-side `webrtc` crate peer connections, SDP negotiation, RTP publication and relay, and track cleanup.

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
- `peer-left`: informs remaining room peers when a participant disconnects.
- `offer`: carries an SDP offer.
- `answer`: carries an SDP answer.
- `candidate`: carries an ICE candidate.
- `relay-error`: reports a relay failure to affected participants.

The server overwrites incoming `room`, `peer_id`, and `from` values with the values associated with the WebSocket connection. Browser SDP and ICE messages terminate at the server. Server-generated SDP and ICE messages are sent back to the browser that owns the corresponding server-side peer connection.

## Browser Call Flow

1. The browser connects to `/ws?room=demo-room` and requests local media.
2. The server sends a `welcome` message with a server-generated peer ID.
3. Browser and server exchange SDP and ICE candidates over the WebSocket to establish the browser-to-server peer connection.
4. The server receives the browser's RTP tracks, creates relay tracks for other room participants, and renegotiates their server-side peer connections.
5. The server forwards encoded audio/video RTP to the other participants. Existing publications are replayed to a participant who joins later.

The microphone icon is displayed over the top-right of the local video. The camera control is displayed next to `Start Call`. Both controls update their icon, visual state, tooltip, and accessibility state when toggled.

## Current Limitations and Security Status

- The browser UI is fixed to `demo-room`; there is no meeting creation flow, room picker, or shareable invite UI.
- Room membership is in memory and is lost when the process exits. There are no accounts, authentication, room authorization, or host roles.
- Room capacity defaults to 2 and is configurable, but a production-supported capacity has not been established. The recorded resource baseline is a short, single-publisher loopback workload, not a production load test.
- WebSocket origin validation, connection/signaling rate limits, and bounded outbound-channel backpressure are not implemented. Do not expose this service publicly as-is.
- The server binds to `0.0.0.0:3000`; TLS termination, graceful shutdown, and deployment configuration are not included.
- RTP is forwarded without decoding or transcoding. RTCP feedback, simulcast/selective forwarding, and production media-quality metrics are not implemented.
- The browser currently uses Google's public STUN server. TURN configuration is not available and may be required on restrictive networks.
- The page loads Vue 3 from unpkg, so initial page rendering requires CDN access.
- Automated browser end-to-end coverage, reconnect recovery, and device selection are not yet implemented.

## Acknowledgements

This project builds on the work of the WebRTC.rs team and the Vue.js team. Their open-source contributions and tooling made the signaling and browser UI layers practical and approachable.

## License

ISC
