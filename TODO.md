# Development TODO

## Product Vision: Zoom/Meet-Style Meetings

Goal: evolve the demo into a reliable, secure meeting app for small groups. Treat the items below as product requirements; keep them unchecked until implemented and verified. Multi-participant media is a prerequisite for the meeting features built on top of it.

## Implementation Sprint: November 9-13, 2026

### Sprint Goal

Deliver a usable guest-based online-meeting MVP as a complete vertical slice: a host creates a meeting, guests join through an invite, the server enforces meeting permissions, participants see and control one another's live media, and the call recovers from common device or network interruptions. Implement and verify working behavior each day; do not mark roadmap items complete based only on design, stubs, or standalone experiments.

This sprint does not claim full production readiness. Use HMAC-SHA-256 signed, expiring meeting tickets with cryptographically random nonces and server-side single-use consumption, plus in-memory meeting state for the MVP; do not introduce a custom account system or pretend guest tickets are user accounts. Keep the default room capacity at 2 unless representative load results justify a change. Scheduling, persistent meeting history, recording, captions, and public deployment remain outside this sprint and stay unchecked.

### Monday, November 9: Meeting Creation and Guest Join

Current status: complete for the implemented MVP foundation. The project now uses a create/join UI and ticketed admission flow, while the broader Tuesday authorization/lobby work remains separate.

- [x] Replace the fixed `demo-room` UI path with a create/join screen accepting a validated display name and either a room code or invite URL.
- [x] Add server endpoints to create a meeting and request admission. Generate unpredictable room IDs and ticket material with a cryptographically secure random source; do not derive identity or credentials from timestamps.
- [x] Return a host credential only to the creator and a distinct, short-lived, single-use guest join ticket after guest admission. Bind each ticket to one room, one role, an expiry, and a nonce; atomically consume its nonce at WebSocket admission. Keep invite URLs limited to the room identifier; exchange the ticket for the WebSocket connection and never log credentials.
- [x] Store meeting owner, members, room lock/lobby settings, and ticket consumption state in shared `AppState`; delete empty meeting state after the final disconnect.
- [x] Keep the existing room-ID validation and capacity enforcement on every admission path, and return actionable HTTP errors for invalid, expired, full, locked, or unknown meetings.
- [x] Add route and service tests for create/join, invalid IDs/names, ticket expiry/reuse, capacity rejection, concurrent admission, and state cleanup. Add UI checks for direct invite-link entry and refresh behavior.
- [x] Exit criteria: host can create a room and copy an invite; a second browser can join by that invite; a third connection is rejected at the current default capacity; invalid or reused tickets cannot open a WebSocket.

### Tuesday, November 10: Authorization, Lobby, and Participant State

- [ ] Authenticate WebSocket upgrades with the one-use join ticket, bind the resulting server-side identity and role to the socket, and ignore client-supplied identity or role fields for all routing and permissions. A lobby guest must remain pending and receive no WebRTC peer connection, media, or room-control permission until a host admits that socket; admission promotes it into the active roster, and denial closes it with a user-readable reason.
- [ ] Enforce configured WebSocket origin allowlists and reject missing/invalid origins in browser-facing production configuration. Fail startup if required signing or origin configuration is absent in production mode.
- [ ] Implement server-authoritative host and guest roles, meeting lock, waiting-room admission/denial, participant removal, and host mute requests. Reject unauthorized actions without mutating state and notify the actor with a structured error.
- [ ] Extend signaling with validated participant display name and microphone, camera, and screen-share state; broadcast updates only within the authorized room and send a current roster snapshot to each admitted participant.
- [ ] Replace unbounded per-socket outbound queues with bounded queues and a defined slow-client policy; add connection and signaling rate limits and limits for names and state payloads.
- [ ] Add WebSocket integration tests for origin rejection, ticket authentication, server-owned identity, host-only moderation, locked/lobby admission, roster snapshots and updates, rate limits, slow consumers, and disconnect cleanup.
- [ ] Exit criteria: a guest cannot admit, remove, mute, lock, or impersonate participants; authorized host operations update every remaining roster consistently; denied requests leave room state unchanged.

### Wednesday, November 11: Complete Participant and Video Experience

- [ ] Replace the demo heading and log-first layout with the meeting room view: room title/invite action, participant roster, responsive local/remote media grid, connection state, and a compact accessible control bar.
- [ ] Render and remove one stable media tile per remote participant/track, show display names and mic/camera/screen-share states, and preserve the local preview when remote tracks change.
- [ ] Add grid and active-speaker layouts plus pin/unpin controls; layout changes must only change presentation and must not renegotiate or stop tracks.
- [ ] Make mic, camera, leave, copy-invite, layout, and pin controls keyboard-operable with accessible names, pressed/selected states, visible focus, and screen-reader status announcements.
- [ ] Handle permission denial and audio-only/video-only capture independently. Present a usable join state when either device is unavailable instead of blocking the entire meeting.
- [ ] Add browser tests for two- and three-participant roster transitions, media tile add/remove, pin/layout changes, keyboard controls, and independent audio/video permission failures.
- [ ] Exit criteria: each participant sees correct names and media states; join/leave updates remove stale tiles; changing layout or pin never interrupts active RTP.

### Thursday, November 12: Screen Sharing, Device Changes, and Recovery

- [ ] Complete screen sharing as a room-wide state transition: replace the outgoing video track, publish the sharer's identity/state, restore the selected camera track on browser capture end, and clean up renegotiation on stop, permission denial, or disconnect.
- [ ] Add camera and microphone device selectors populated from `enumerateDevices`; apply device changes to active senders without losing the meeting, and handle device removal or permission revocation with a clear fallback state.
- [ ] Implement bounded exponential WebSocket reconnect with visible reconnecting/failed states, rejoin using a fresh ticket, refresh roster state, and recreate/re-negotiate the peer connection without duplicating media tiles or forwarding tasks.
- [ ] Recover from failed ICE/peer-connection state with a bounded ICE restart or controlled peer-connection rebuild; stop retrying and show a recoverable leave/rejoin action after the retry budget is exhausted.
- [ ] Add tests for screen-share start/end and browser-ended capture, device replacement/removal, WebSocket interruption, ICE failure, successful recovery, exhausted retries, and cleanup after leave during recovery.
- [ ] Exit criteria: screen capture ending returns to camera when available; device changes do not create duplicate tracks; temporary transport loss restores the call and roster, while permanent failure presents a clear action and leaves no server-side peer or relay state.

### Friday, November 13: End-to-End Acceptance and MVP Closeout

- [ ] Run the complete browser flow with automated Chromium peers using fake media devices: create room, join by invite, exchange audio/video, share/stop screen, exercise host admission/removal/mute, disconnect/reconnect, and leave/cleanup.
- [ ] Add a four-participant relay acceptance workload in an optimized server build with every participant publishing audio and video; verify every expected subscriber receives media, negotiation and teardown complete, and no peer/publication/relay/task state remains after room shutdown.
- [ ] Keep the default capacity at 2 unless the four-participant workload passes repeatedly with documented host/build conditions and measured CPU, memory, packet delivery, and cleanup. If it does not pass, fix the relay issue or keep capacity unchanged and document the blocker; do not infer capacity from the current single-publisher baseline.
- [ ] Run `cargo fmt --check`, `cargo check`, `cargo test`, the browser end-to-end suite, and the optimized multi-participant acceptance workload. Fix regressions in this sprint's scope before calling the MVP complete.
- [ ] Update `PROGRESS.md` with shipped behavior, exact commands/results, known limitations, any incomplete criteria, and the next release-blocking work. Update `CLAUDE.md` to reflect the actual media-routing and security architecture.
- [ ] Exit criteria: a fresh checkout can be configured and run from documented steps; automated tests prove the full guest meeting workflow and cleanup; no production-readiness checklist item is marked complete without its own evidence.

### Sprint-Wide Definition of Done

- [ ] No credential or role is trusted from an unverified client field; room-scoped actions are authorized server-side.
- [ ] Every new signaling message has Rust serialization/validation coverage and a browser integration test.
- [ ] Media, roster state, and moderation state converge correctly after join, leave, denial, reconnect, and host action.
- [ ] Bounded retries and queues prevent unbounded task, memory, or signaling growth; all exits clean up peer connections, relay tracks, and forwarding tasks.
- [ ] Accessible controls and understandable permission, connection, and media failure states are covered by browser checks.
- [ ] The sprint report distinguishes implemented guest-MVP behavior from account authentication, persistence, TLS/TURN deployment, scheduling, captions, recording, and any unverified room-capacity claims.

### P0: Multi-Participant Meeting Foundation

- [ ] Measure server resources and choose a supported production room capacity; keep the default at 2 until then.
- [ ] Complete and load-test SFU track publication, subscription, renegotiation, and cleanup for multiple participants.
- [x] Verify late-join track replay and multi-participant RTP delivery with a three-peer negotiation test.
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
- [x] Retain published tracks for late subscribers and render one remote tile per stream; three-peer RTP replay is verified by the November 5 loopback test.
- [x] Queue server renegotiation changes and roll back colliding offers; collision rollback and queued-offer behavior were verified November 4, and multi-peer RTP delivery was verified November 5.

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

## Four-Day Scope: November 3-6, 2026 (Completed)

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

### Friday, November 6: Integration and Closeout (Completed)

- [x] Finish focused unit and WebSocket integration tests for all three limits.
- [x] Document the room ID rules, peer limit, message-size limit, and rejection behavior.
- [x] Run `cargo fmt --check`, `cargo check`, and `cargo test`; resolve issues within this scope.

#### Friday, November 6, 2026: Resource Baseline and Capacity Decision

Goal: measure the current relay workload before selecting a supported production room capacity. Keep the default room limit at 2 unless repeatable measurements justify a change.

- [x] Define a repeatable loopback workload with fixed participant counts, synthetic RTP packet rate, payload size, and run duration.
- [x] Measure 2- and 3-participant runs repeatedly; record host details, CPU time, peak memory, and sent/received packet counts.
- [x] Verify each run delivers media as expected and leaves no connections, publications, relay tracks, or forwarding tasks behind.
- [x] Record the measurements, test limitations, and a provisional room-capacity recommendation in `PROGRESS.md`.
- [x] Run `cargo fmt --check`, `cargo check`, and `cargo test` after the benchmark changes.

### November 4: P0 Relay Stabilization

Focus: fix the signaling and relay path so one room can safely handle offer/answer negotiation, track lifecycle, and teardown without races.

- [x] Add a minimal local SDP offer/answer regression test through `WebRtcAgent`.
- [x] Verify per-peer SDP serialization and queued renegotiation under concurrent audio/video track changes.
- [x] Test queued-offer handling after an outstanding server offer is answered.
- [x] Test SDP rollback when a browser offer collides with a server offer.
- [x] Verify tracks published before a new participant joins are delivered to that participant.
- [x] Remove relay tracks from active peer connections when a publisher leaves.
- [x] Track and cancel RTP forwarding tasks during disconnects.
- [x] Signal relay setup and RTP-write failures to affected peers and show them in the browser UI.
- [x] Reject source codec mappings absent from the destination offer.
- [x] Re-run focused relay tests, `cargo fmt --check`, `cargo check`, and `cargo test`.

Late-join replay, RTP forwarding, and publisher teardown were verified by the November 5 three-peer loopback integration test.

### November 5: Three-Peer RTP Verification

Goal: prove the relay carries media across a three-peer room and cleans up subscriber tracks and forwarding tasks when a publisher disconnects. Use local peer connections and loopback ICE so the test does not depend on cameras, external STUN, or browser automation.

- [x] Build a reusable local-peer test harness that exchanges ICE candidates and observes remote tracks.
- [x] Publish a track from peer A, then join peer C; verify the server replays A's existing publication into C's initial negotiation.
- [x] Send representative RTP from A and verify both peer B and late-joining peer C receive packets.
- [x] Disconnect publisher A; verify its relay senders are removed from active subscribers and renegotiation is triggered or queued.
- [x] Verify RTP forwarding tasks for A stop and publication/relay state is removed after teardown.
- [x] Run the focused three-peer test repeatedly, then run `cargo fmt --check`, `cargo check`, and `cargo test`.

Exit criteria: all media and cleanup assertions pass in the loopback integration test. If real RTP cannot be made deterministic with the current `webrtc` test APIs, record the blocker and keep these items unchecked rather than substituting SDP-only coverage.

## P0: Make the Relay Correct

- [x] Serialize SDP renegotiation per peer connection.
- [x] Add offer queues and signaling-state handling for concurrent track changes.
- [x] Handle SDP rollback when offer collisions occur.
- [x] Add tracks published before a new participant joins to that participant.
- [x] Remove relay tracks from active peer connections when a publisher leaves.
- [x] Track and cancel RTP forwarding tasks during disconnects.
- [x] Handle relay failures and notify affected peers.
- [x] Verify that source codecs are supported by each destination peer.

## P1: Improve Media Quality

- [ ] Forward and consume RTCP feedback, including NACK and PLI.
- [ ] Read outgoing RTCP packets so sender feedback tasks do not stall.
- [ ] Preserve correct SSRC and RTP header-extension mappings per destination.
- [ ] Add bitrate, packet-loss, jitter, and connection-state monitoring.
- [ ] Test camera, microphone, and screen-share transitions during active calls.

## P1: Secure the Service

- [ ] Add authentication and room authorization.
- [ ] Validate WebSocket origins.
- [x] Validate and constrain room IDs.
- [x] Add room capacity limits.
- [x] Add WebSocket message-size limits.
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
- [x] Display clear connection, relay, and media errors.
- [x] Support multiple remote participants instead of one remote video element.
- [ ] Improve screen-share state recovery after browser capture ends.
- [ ] Add responsive controls and production-quality accessibility review.

## P2: Testing

- [x] Add unit tests for signaling parsing, candidate serialization, and room ID validation.
- [x] Add integration tests for server-side offer/answer negotiation.
- [ ] Add RTP relay tests for audio and video tracks.
- [ ] Add screen-share renegotiation tests.
- [ ] Add disconnect, reconnect, and cleanup tests.
- [x] Add multi-participant room tests.
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
