# WebRTC Axum Media Relay

A small-group meeting prototype built with Rust, Axum, Tokio, and the `webrtc` crate. It serves an embedded browser UI, manages room membership and signaling over WebSockets, and relays encoded RTP through server-side WebRTC peer connections. It is not yet a production-ready meeting service.

Each browser establishes a WebRTC connection to the Rust server. The server forwards incoming RTP to the other participants in the room; media is not sent directly between browsers.

## Features

- Axum HTTP server with `/`, `/health`, and room-scoped `/ws` endpoints
- Server-assigned peer identities and in-memory room membership
- Server-side SDP offer/answer handling, ICE exchange, and queued renegotiation
- RTP forwarding between participants through server-side peer connections
- Late-join replay of existing publications
- Embedded HTML/CSS/JavaScript meeting UI (no frontend CDN dependency)
- Meeting creation and guest admission using short-lived join tickets
- Participant roster, local preview, remote media tiles, and mic/camera controls
- Grid and active-speaker presentation with pin controls
- Per-room peer limit, configurable with `MAX_PEERS_PER_ROOM` (default: 4; not yet load-validated)
- Maximum inbound WebSocket message size of 64 KiB
- `/health` endpoint

## Requirements

- Rust toolchain with Cargo
- A modern browser supporting:
  - `navigator.mediaDevices.getUserMedia`
  - `navigator.mediaDevices.getDisplayMedia` for screen sharing
  - `RTCPeerConnection`
  - WebSocket
- Internet access to reach the configured STUN server
- Camera and microphone permissions for full media; either device can be unavailable independently

## Run

```sh
cargo run
```

The server binds to `0.0.0.0:3000`; open the app locally at:

```text
http://127.0.0.1:3000
```

The configured room peer limit defaults to 4. It can be changed for local testing, for example:

```sh
MAX_PEERS_PER_ROOM=3 cargo run
```

Open the app and enter a display name. Create a room to receive a host invite, or join with a room code or invite link. The host invite is copied without exposing the host credential; guest admission requests a short-lived ticket before opening the signaling socket. Open a second browser tab to join and test the two-participant media flow. The UI shows local and remote media, participant state, connection status, and meeting controls. Screen sharing and reconnect recovery are not part of the current UI flow.

For signaling-level testing, connect to the WebSocket endpoint with a valid room ID and an issued ticket:

```text
ws://127.0.0.1:3000/ws?room=my-room&ticket=<issued-ticket>
```

Meetings and tickets are held in memory and disappear when the server exits. The default configured room capacity is 4, but representative four-participant production load has not been validated; do not treat this setting as a supported-capacity claim.

## Room and WebSocket Limits

The server enforces room ID, room capacity, and inbound WebSocket message limits:

- Room IDs must be 1-64 characters long.
- Allowed characters are ASCII letters, numbers, `-`, and `_`.
- Whitespace and separators like `/` are rejected.
- Each room admits at most `MAX_PEERS_PER_ROOM` peers; the configured default is 4.
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

The Chromium browser suite covers three-participant audio/video delivery, roster and media-tile cleanup, layout and pin changes without interrupting RTP, keyboard controls, mobile-width layout, and independent audio/video permission failures. Install its dependencies and browser once, then run it with:

```sh
npm ci
npx playwright install chromium
npm run test:browser
```

The Playwright web server starts the application with `MAX_PEERS_PER_ROOM=3` for these local tests; the normal server default is 4.

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
- `src/ui.rs`: Embedded HTML, CSS, and browser-side WebRTC logic.
- `src/webrtc.rs`: Server-side `webrtc` crate peer connections, SDP negotiation, RTP publication and relay, and track cleanup.

## Signaling Messages

Messages use the following structure:

```json
{
  "type": "offer",
  "room": "sample-room",
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

1. A host creates a meeting or a guest requests admission to an existing meeting.
2. The browser opens `/ws` using the room ID and its host or guest ticket, then receives a `welcome` message with a server-generated peer ID.
3. Browser and server exchange SDP and ICE candidates over the WebSocket to establish the browser-to-server peer connection.
4. The server receives the browser's RTP tracks, creates relay tracks for other room participants, and renegotiates their server-side peer connections.
5. The server forwards encoded audio/video RTP to the other participants. Existing publications are replayed to a participant who joins later.

Microphone and camera controls appear in the meeting control bar. Roster labels and media tiles update from participant state and remote tracks; layout and pin controls do not modify media tracks.

## Current Limitations and Security Status

- Room membership is in memory and is lost when the process exits. There are no persistent accounts or meeting history.
- Room capacity defaults to 4 by product decision and is configurable, but a production-supported capacity has not been established. The recorded resource baseline is a short, single-publisher loopback workload, not a production load test.
- Guest tickets and host roles are implemented, but this prototype is not a substitute for account authentication or production security review. Do not expose this service publicly as-is.
- The server binds to `0.0.0.0:3000`; TLS termination, graceful shutdown, and deployment configuration are not included.
- RTP is forwarded without decoding or transcoding. RTCP feedback, simulcast/selective forwarding, and production media-quality metrics are not implemented.
- The browser currently uses Google's public STUN server. TURN configuration is not available and may be required on restrictive networks.
- The Chromium suite covers participant media and controls, but full sprint acceptance for moderation, reconnect recovery, device selection, and screen sharing remains outstanding.

## Acknowledgements

This project builds on the work of the WebRTC.rs team and the Rust and browser API communities. Their open-source contributions and tooling made the signaling and media layers practical and approachable.

## License

ISC
