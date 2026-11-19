# CLAUDE.md

## Project Overview

`Garay` (Gather Relay) is a Rust WebRTC meeting prototype that now includes a guest-meeting flow, room admission, role-aware moderation, and browser-based media negotiation. The project is designed as a small-group meeting app rather than a simple signaling demo.

The server owns room membership, ticket-based admission, and the canonical peer identity for each socket. Browsers negotiate media using native `RTCPeerConnection`; the server runs an in-process SFU-style relay (`WebRtcAgent` in `webrtc.rs`) that forwards RTP packets between participants. The Rust-side `webrtc` peer connections encode and apply incoming SDP/ICE for each room/peer relationship, track lifecycle, and time out stale connection state.

## Current Product Reality

This project is a verified guest-meeting MVP slice, not a production-ready public deployment. It supports:

- room creation and guest invitation flow
- short-lived single-use join tickets
- host/guest role binding on sockets
- lobby and room-lock behavior
- server-side roster updates and state broadcast
- screen sharing, device switching, reconnect recovery, and peer rebuild logic
- browser automation coverage for the core meeting flow
- **four-participant all-publishers acceptance workload with CPU/memory measurement (validated 2026-11-19)**

The remaining gaps are explicit: release-level deployment hardening, full production-readiness documentation, and validating delivery quality under network impairment.

## Technology

- Rust edition 2024
- Tokio async runtime
- Axum 0.8 with WebSocket support
- `webrtc` 0.20.5
- Serde and Serde JSON for signaling payloads
- `futures-util` for WebSocket stream and sink handling
- Playwright browser tests for UI and migration flow validation

## Commands

Run from the repository root:

```sh
cargo fmt --check
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

The browser tests use the project’s local Playwright setup:

```sh
npm ci
npx playwright install chromium
npm run test:browser -- --reporter=list
```

If a previous development process owns port 3000, identify or stop it before restarting.

## Architecture

### Overview

The system has two main planes:

- **Signaling plane:** browsers and server exchange JSON messages over WebSockets; the server owns room membership and routing decisions.
- **Media plane:** browser `RTCPeerConnection`s connect to a server-side SFU relay (`WebRtcAgent`); the Rust process forwards RTP packets between participants (it does not decode media).

The server-side `webrtc` peer connections encode and apply incoming SDP/ICE for each room/peer relationship, track lifecycle, and time out stale connection state. Each participant publishes tracks to the relay, which fans out to all other room members.

```text
Browser A ── WS /ws?room=R&ticket=T ──┐        ┌── WS /ws?room=R&ticket=T ── Browser B
                                      │
                                      ▼
                              app.rs (meeting, room, auth, routing)
                                      │
                                      ▼
                  webrtc.rs (WebRtcAgent: server-side relay per room)

Browser A ── RTP ──► WebRtcAgent ── RTP ──► Browser B
Browser A ◄── RTP ── WebRtcAgent ◄── RTP ── Browser B
```

### Module Responsibilities

- `src/main.rs`: Tokio entrypoint; launches the Axum app.
- `src/app.rs`: meeting creation, guest ticket handling, room membership, server-side identity, origin checks, moderation, and WebSocket lifecycle.
- `src/signaling.rs`: signaling wire format, room validation, ICE candidate support, and participant-state schemas.
- `src/ui.rs`: embedded meeting browser UI, create/join view, room controls, media negotiation, and recovery logic.
- `src/webrtc.rs`: server-side WebRTC peer storage, SDP/ICE processing, **SFU relay (WebRtcAgent)**, track publication/fanout, RTP forwarding tasks, and teardown cleanup.

### Current Meeting Lifecycle

1. A browser opens the app at `/` and enters a display name.
2. The user creates a room or joins via a room code or invite URL.
3. The server issues a host ticket for the creator or a single-use guest ticket for invited users.
4. The browser connects to `/ws?room=<room>&ticket=<ticket>`.
5. The server authenticates the ticket, binds the socket to the canonical room and role, and issues a `welcome` event with the assigned peer ID.
6. Existing room members and the new participant are synchronized through roster updates.
7. Offers, answers, and ICE candidates are routed only to the intended target peer.
8. On disconnect, the server removes the participant and cleans up peer state and relay entries.

### Invariants

- The server owns and overwrites `room`, `peer_id`, and `from` values from the socket context.
- Client-generated IDs are never trusted for routing.
- SDP and ICE are targeted with `to`; they are never broadcast to the whole room.
- Every room admission path enforces the configured room capacity.
- Meeting state and tickets are server-side and in-memory for the MVP.

## Security and Admission Model

This is still a prototype-grade security model, but it is stronger than the original demo and is intentionally audited in the sprint plan.

### Trusted versus untrusted data

- **Untrusted:** all client-provided fields in WebSocket JSON, any room identifiers, any role or identity claims, SDP, and ICE data.
- **Trusted:** the server-owned association between a socket and `(room_id, peer_id, role)` after authentication and admission.

### Current protections

- Single-use join tickets with role, room, expiry, and nonce data
- Ticket consumption tracking to prevent replay
- Host-only actions enforced at the server
- Origin allowlisting for browser-facing WebSocket upgrades
- Room cap enforcement with `MAX_PEERS_PER_ROOM`
- Message payload and state limits for display names and signaling state
- Bounded outbound socket queues and signaling-rate protections
- Targeted forwarding so SDP/ICE do not reach unrelated peers
- Server-side RTP relay (`WebRtcAgent`) fans out tracks only to room members

### Known gaps and limitations

- This is not a public-production deployment and is not hardened for hostile internet exposure.
- TLS and `wss://` are not yet configured.
- TURN configuration is not yet production-ready for restrictive NAT/firewall environments.
- The default `MAX_PEERS_PER_ROOM = 4` has been functionally validated with a four-participant all-publishers workload (3 debug + 3 release runs passing). **Capacity validation evidence exists but has not been stress-tested under network impairment or sustained load.**
- The current architecture still depends on in-memory state and is not a persistent multi-tenant service.
- Authentication is intentionally not a full user account system; join tickets are short-lived meeting credentials only.
- Browser test environment has a `libsimdjson` version mismatch requiring manual fix.

## HTTP and WebSocket API

### Routes

- `GET /`: serves the embedded meeting UI
- `GET /health`: returns server health JSON
- `POST /api/meeting`: creates a room and returns a host ticket and invite URL
- `POST /api/meeting/:room/ticket`: issues a guest join ticket
- `GET /ws?room=<room>&ticket=<ticket>`: upgrades a socket after ticket validation

### Message model

`SignalMessage` is the main JSON payload. Important fields include:

- `type`: `welcome`, `peer-joined`, `peer-left`, `offer`, `answer`, `candidate`, `relay-error`, and related room-state updates
- `room`: server-normalized room ID
- `peer_id`: server-assigned sender ID
- `from`: sender ID as tracked by the server
- `to`: target peer ID; `""` means broadcast to the room for notifications
- `sdp`: SDP payload for offer/answer exchange
- `candidate`: ICE candidate payload

The server overwrites any client-supplied `room`, `peer_id`, or `from` values before routing. The browser cannot control server-side identity or room membership through untrusted fields.

## Browser Negotiation Flow

1. The user creates or joins a room through the embedded UI.
2. The browser connects to the WebSocket using a valid room and ticket.
3. The server sends `welcome` with the canonical peer ID and state snapshot metadata.
4. Room members receive `peer-joined` or roster updates when participants arrive or leave.
5. The browser with the smaller peer ID starts the offer.
6. The other browser creates and returns an answer.
7. ICE candidates are exchanged with `to` targeting the peer.
8. Local camera/microphone/screen-share tracks are added to the browser `RTCPeerConnection` and remote tracks are displayed in the meeting UI.

The browser UI also supports toggling microphone/camera, screen-share transitions, reconnect recovery, layout changes, and pinning without stopping media negotiation.

## Implementation Guidelines

- Keep routing and room-state logic in `app.rs`.
- Keep signaling wire types in `signaling.rs`.
- Keep browser logic in `ui.rs`.
- Keep server-side peer connection and relay behavior in `webrtc.rs`.
- Match the installed `webrtc` 0.20.5 API carefully; do not assume behavior from older examples.
- Keep SDP/ICE targeted with `to` whenever the message belongs to one peer.
- Update both the Rust signal model and browser payload serialization together when a signaling shape changes.
- Run `cargo fmt --check`, `cargo check`, and `cargo test` after Rust changes.
- Do not commit build artifacts from `target/`.

## Known Limitations

- The project is still local-development oriented and not a public-facing production deployment.
- The default room capacity of 4 has been functionally validated with a four-participant all-publishers acceptance workload (6 test runs: 3 debug, 3 release — all passing). **Release profile: ~7.2s total CPU, ~87 MB peak RSS. Delivery: ~21k packets per media type over 10s (0% loss vs 6k expected per peer).** Stress testing under packet loss, jitter, and sustained load has not been performed.
- The UI is embedded and local to the project; it does not yet provide a full multi-tenant or externally hosted deployment story.
- Browser media still depends on device permissions and the configured STUN path.
- Real production reliability requires TLS/TURN, deployment configuration, and deeper operations tooling.
- Meeting history, scheduling, captions, recording, and user-account features remain outside this MVP scope.
- Browser automation environment has a `libsimdjson` version mismatch (node linked against .36, system has .37) requiring manual symlink fix.

## Current Status Summary

The app is in a verified guest-meeting MVP state for the current offline/local smoke flow, with working Rust and browser automation coverage for the implemented features.

**Phase 1 (Reliability and capacity proof) — COMPLETE:**
- Four-participant all-publishers acceptance workload implemented in `src/webrtc.rs` (`four_peer_all_publishers_acceptance`)
- Validated in both debug and release profiles (3 runs each, all passing)
- CPU/memory measurement via `libc::getrusage` at 6 phases (baseline, after ICE, after tracks published, after offers answered, after receive window, after cleanup)
- Delivery quality metrics: per-peer packet loss %, bitrate (kbps) calculation
- Release profile: ~7.2s CPU (user+sys), ~87 MB peak RSS
- Debug profile: ~31.0s CPU, ~87 MB peak RSS
- Cleanup verification: all relay_tasks, published_tracks, relays, connections, senders empty after teardown

**Phase 2 (Operational readiness) — COMPLETE:**
- Structured JSON logging via `tracing`/`tracing-subscriber` with configurable format (json/text) and level
- Prometheus metrics exporter at `/metrics` with counters, gauges, histograms for:
  - WebSocket connections (active, total, errors)
  - Signaling messages (received/sent by type)
  - Room/peer lifecycle (created, joined, left, capacity rejections)
  - WebRTC negotiation duration, peer connection establishment
  - Media relay (tracks published, RTP packets forwarded, relay errors)
  - Resource usage (CPU time, RSS)
  - HTTP requests (latency, status codes)
- Health/readiness/liveness endpoints (`/health`, `/ready`, `/live`) for Kubernetes deployments
- Graceful shutdown with connection draining via `tokio::signal::ctrl_c` and Axum's `with_graceful_shutdown`
- Configuration management via `config` crate (TOML + `GARAY__` env vars) in `src/config.rs`
- TLS configuration infrastructure (cert/key loading, self-signed generation for dev) - reverse proxy termination recommended for production
- TURN/STUN configuration via `/api/ice-config` endpoint
- All 20 unit tests pass; four-peer acceptance test passes in debug and release

**Remaining work:** Phase 3 (Documentation and launch gate — deployment docs, Dockerfile, docker-compose, systemd service, config.toml.example, TURN server setup guide, TLS reverse proxy config). Browser test environment needs `libsimdjson` fix.
