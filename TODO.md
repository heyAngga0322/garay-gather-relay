# Development TODO

## Product Vision: Zoom/Meet-Style Meetings

Goal: evolve the demo into a reliable, secure meeting app for small groups. Treat the items below as product requirements; keep them unchecked until implemented and verified. Multi-participant media is a prerequisite for the meeting features built on top of it.

### P0: Multi-Participant Meeting Foundation

- [ ] Measure server resources and choose a supported production room capacity; keep the default at 2 until then.
- [ ] Complete and load-test SFU track publication, subscription, renegotiation, and cleanup for multiple participants.
- [ ] Verify late-join track replay and multi-participant RTP delivery with a three-peer negotiation test.
- [ ] Show a participant roster with join/leave and microphone/camera state.
- [ ] Render all remote participants and the local preview in responsive grid and active-speaker layouts.
- [ ] Allow participants to pin a video and switch layouts without disrupting media.
- [ ] Provide meeting creation and joining by room name or shareable invite link; remove the fixed `demo-room` default from the UI.
- [ ] Add host/co-host roles, host transfer, and permission checks for moderation actions.
- [ ] Add a waiting room, host admission/denial, meeting lock, participant removal, and host mute controls.
- [ ] Handle device selection, device changes, permission denial, and browser capture ending without losing the meeting state.
- [ ] Recover from temporary WebSocket and WebRTC failures, with clear reconnecting and failed states.

#### Progress Recorded November 3, 2026

- [x] Make room capacity configurable with `MAX_PEERS_PER_ROOM`, keep the default at 2, and test a configured capacity of 3.
- [x] Broadcast `peer-left` and verify the notification with a WebSocket integration test.
- [x] Accept the targeted `welcome` signal before the client knows its assigned peer ID; confirm the browser sends an offer and receives an answer.
- [x] Serialize ICE candidate metadata as browser-standard `sdpMid` and `sdpMLineIndex`; add a serialization regression test.
- [ ] Retain published tracks for late subscribers and render one remote tile per stream; implementation is present, but three-peer RTP behavior is not yet verified.
- [ ] Queue server renegotiation changes and roll back colliding offers; collision rollback and queued-offer behavior were verified November 4, while multi-peer RTP delivery remains unverified.

### P1: Core In-Call Collaboration

- [ ] Add meeting chat with server-side authorization, message limits, and a documented retention policy.
- [ ] Add hand raising and lightweight reactions, visible in the participant roster and meeting view.
- [ ] Add captions, including clear indication when captions are unavailable or disabled.
- [ ] Make screen sharing available and understandable to participants, including who is sharing and when sharing ends.
- [ ] Add keyboard-accessible meeting controls and review the full meeting flow for screen-reader accessibility.
- [ ] Show actionable network and media health states without exposing sensitive connection details.

### P2: Meeting Lifecycle and Advanced Features

- [ ] Support scheduled meetings, time zones, and calendar invite export.
- [ ] Support meeting passcodes and configurable lobby/guest policies.
- [ ] Add recording only after explicit participant notice and consent, with access control, storage limits, and deletion rules.
- [ ] Add host-configurable meeting options, including who can share their screen and send chat messages.
- [ ] Define and implement privacy, data retention, and account deletion behavior for user and meeting data.

### Release Gates: Production Readiness

- [ ] Verify authentication, room authorization, host permissions, origin checks, and abuse limits before public deployment.
- [ ] Define supported browsers, room sizes, and service-level targets for call setup and media quality.
- [ ] Pass browser end-to-end tests for joining, multi-party audio/video, screen sharing, moderation, reconnect, and cleanup.
- [ ] Pass load and failure tests at the documented room capacity, including packet loss and server restart scenarios.
- [ ] Add deployment, TLS/TURN, secrets, scaling, backup, incident response, and rollback runbooks.
- [ ] Provide privacy information and a user-facing way to report meeting abuse or technical issues.

## Planned Four-Day Scope (November 3-6, 2026)

Scope: constrain room IDs, enforce a per-room peer limit, limit WebSocket message size, and add focused tests and documentation. Authentication, origin validation, rate limiting, and outbound-channel backpressure are out of scope for this schedule. Keep the checklist below unchecked until each item is implemented and verified.

### Tuesday, November 3: Room ID Rules

- [x] Define allowed room ID characters and length.
- [x] Validate room IDs before accepting a WebSocket connection.
- [x] Add unit tests for valid, empty, oversized, and malformed room IDs.

### Wednesday, November 4: Room Capacity

- [x] Define a per-room peer limit and reject connections that exceed it.
- [x] Ensure concurrent joins cannot exceed the limit and rejected peers do not leave stale room state.
- [x] Add focused tests for admission, capacity rejection, and cleanup.

### Thursday, November 5: WebSocket Message Limit

- [x] Configure a maximum inbound WebSocket message size.
- [x] Verify oversized messages are rejected and normal signaling messages still work.
- [x] Add WebSocket-level coverage for the size limit and room constraints.

### Friday, November 6: Integration and Closeout

- [x] Finish focused unit and WebSocket integration tests for all three limits.
- [x] Document the room ID rules, peer limit, message-size limit, and rejection behavior.
- [x] Run `cargo fmt --check`, `cargo check`, and `cargo test`; resolve issues within this scope.

### November 4: P0 Relay Stabilization

Focus: fix the signaling and relay path so one room can safely handle offer/answer negotiation, track lifecycle, and teardown without races.

- [x] Add a minimal local SDP offer/answer regression test through `WebRtcAgent`.
- [x] Verify per-peer SDP serialization and queued renegotiation under concurrent audio/video track changes.
- [x] Test queued-offer handling after an outstanding server offer is answered.
- [x] Test SDP rollback when a browser offer collides with a server offer.
- [ ] Verify tracks published before a new participant joins are delivered to that participant.
- [ ] Remove relay tracks from active peer connections when a publisher leaves.
- [ ] Track and cancel RTP forwarding tasks during disconnects.
- [x] Signal relay setup and RTP-write failures to affected peers and show them in the browser UI.
- [x] Reject source codec mappings absent from the destination offer.
- [x] Re-run focused relay tests, `cargo fmt --check`, `cargo check`, and `cargo test`.

Note: late-join replay, RTP forwarding, and publisher teardown still need a multi-peer ICE/RTP integration test before those checklist items can be closed.

## P0: Make the Relay Correct

- [ ] Serialize SDP renegotiation per peer connection.
- [ ] Add offer queues and signaling-state handling for concurrent track changes.
- [ ] Handle SDP rollback when offer collisions occur.
- [ ] Add tracks published before a new participant joins to that participant.
- [ ] Remove relay tracks from active peer connections when a publisher leaves.
- [ ] Track and cancel RTP forwarding tasks during disconnects.
- [ ] Handle relay failures and notify affected peers.
- [ ] Verify that source codecs are supported by each destination peer.

## P1: Improve Media Quality

- [ ] Forward and consume RTCP feedback, including NACK and PLI.
- [ ] Read outgoing RTCP packets so sender feedback tasks do not stall.
- [ ] Preserve correct SSRC and RTP header-extension mappings per destination.
- [ ] Add bitrate, packet-loss, jitter, and connection-state monitoring.
- [ ] Test camera, microphone, and screen-share transitions during active calls.

## P1: Secure the Service

- [ ] Add authentication and room authorization.
- [ ] Validate WebSocket origins.
- [ ] Validate and constrain room IDs.
- [ ] Add room capacity limits.
- [ ] Add WebSocket message-size limits.
- [ ] Add connection, signaling, and screen-share rate limits.
- [ ] Prevent unbounded outbound channel memory growth.
- [ ] Add abuse protection for repeated connections and malformed signaling.

## P1: Production Networking

- [ ] Add HTTPS and secure WebSockets (`wss://`).
- [ ] Make STUN and TURN servers configurable through environment variables.
- [ ] Add TURN support for restrictive NATs and firewalls.
- [ ] Make UDP bind addresses and port ranges configurable.
- [ ] Document firewall, reverse-proxy, and NAT requirements.
- [ ] Add graceful shutdown and connection draining.

## P2: Reliability and Operations

- [ ] Replace `eprintln!` with structured tracing.
- [ ] Add metrics for rooms, peers, tracks, relay tasks, RTP packets, and failures.
- [ ] Add readiness and liveness endpoints.
- [ ] Remove startup `unwrap()` calls and return actionable errors.
- [ ] Add configuration validation at startup.
- [ ] Define resource limits for rooms, peers, tracks, and task counts.
- [ ] Add deployment documentation and container configuration.

## P2: Client Product Experience

- [ ] Make the room ID configurable instead of hardcoding `demo-room`.
- [ ] Add WebSocket reconnect and peer-connection recovery.
- [ ] Handle camera-only, microphone-only, and permission-denied states separately.
- [ ] Display clear connection, relay, and media errors.
- [ ] Support multiple remote participants instead of one remote video element.
- [ ] Improve screen-share state recovery after browser capture ends.
- [ ] Add responsive controls and production-quality accessibility review.

## P2: Testing

- [ ] Add unit tests for signaling normalization and validation.
- [ ] Add integration tests for server-side offer/answer negotiation.
- [ ] Add RTP relay tests for audio and video tracks.
- [ ] Add screen-share renegotiation tests.
- [ ] Add disconnect, reconnect, and cleanup tests.
- [ ] Add multi-participant room tests.
- [ ] Add browser automation tests for camera, microphone, and screen sharing.
- [ ] Add packet-loss and network-failure tests.
- [ ] Add load tests for room and peer limits.

## P3: Scale the Architecture

- [ ] Define SFU room and track ownership abstractions.
- [ ] Add backpressure-aware packet fanout.
- [ ] Support simulcast and selective forwarding by subscriber quality.
- [ ] Add horizontal scaling strategy and shared room coordination.
- [ ] Add distributed session routing or a media gateway layer.
- [ ] Measure CPU, memory, bandwidth, and latency under representative load.
