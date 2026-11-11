# Progress Report: November 5, 2026

## Three-Peer RTP Verification

Completed the November 5 loopback verification plan. The test uses three local WebRTC peers and exchanges ICE candidates without cameras, external STUN, or browser automation.

- Peer A publishes synthetic VP8 RTP; peer B receives packets through the relay.
- Peer C joins after A has published. The server replays A's existing publication during C's initial negotiation, and C receives RTP packets.
- After A disconnects, the server removes both subscriber relay senders and requests renegotiation. The publication and relay-task registry entries are cleared, and no additional RTP arrives at B or C.
- The focused test passed five consecutive runs.

## Fixes Made

`TrackRemote::poll()` yields an `OnOpen` event before RTP. The relay loop previously treated any non-RTP event as termination, which immediately removed a newly registered publication. It now ignores non-terminal setup, mute, unmute, and RTCP events, and stops only on track termination, errors, or stream closure.

Server peer connections now register the default media codecs needed for relay senders. A localhost UDP bind option is limited to tests so the integration harness exercises loopback ICE while production keeps its existing default bind behavior.

## Validation

- `cargo fmt --check` passed.
- `cargo check` passed.
- `cargo test` passed: 15 tests, 0 failures.
- `three_peer_rtp_replay_and_publisher_cleanup` passed five consecutive focused runs, including the teardown silence assertions.

## Status

The November 5 checklist and its overlapping late-join, relay removal, and forwarding-task items are marked complete in `TODO.md`. This verifies the loopback test scenario; it does not replace future capacity load tests or production network/browser end-to-end testing.

# Progress Report: November 6, 2026

## Resource Baseline and Capacity Decision

Added opt-in loopback resource benchmarks in `src/webrtc.rs` for two and three participants. Each run has one VP8 publisher and one or two receiving peers, sends 1,200-byte synthetic RTP payloads at 50 packets/second for 10 seconds, and runs in its own test process. A continuous, uncounted warm-up stream triggers the server's `on_track` publication path. The measured workload then checks payload size and delivery counts, closes every peer, and asserts that connection, sender, publication, relay, and relay-task registries are empty.

The ignored test names are `webrtc::tests::resource_baseline_two_peers` and `webrtc::tests::resource_baseline_three_peers`. Run each three times to reproduce the samples below:

```sh
cargo test -- --ignored --exact webrtc::tests::resource_baseline_two_peers --nocapture
cargo test -- --ignored --exact webrtc::tests::resource_baseline_three_peers --nocapture
```

Host: AMD Ryzen 7 3700U with Radeon Vega Mobile Gfx, x86_64, CachyOS kernel `7.3.1-1-cachyos`, 14,175,116 KiB RAM. Results are from Cargo's debug test profile. CPU is process user plus system CPU time for setup, negotiation, workload, and cleanup. Peak RSS is the test process's `getrusage` high-water mark, not a server-only or incremental measurement.

| Participants | Run | Sent | Received by each subscriber | User CPU (s) | System CPU (s) | Peak RSS (MiB) | Elapsed (s) |
| --- | ---: | ---: | --- | ---: | ---: | ---: | ---: |
| 2 | 1 | 500 | B: 498 | 1.802 | 0.205 | 72.04 | 10.308 |
| 2 | 2 | 500 | B: 498 | 1.621 | 0.202 | 71.43 | 10.310 |
| 2 | 3 | 500 | B: 498 | 1.524 | 0.228 | 71.87 | 10.323 |
| 3 | 1 | 500 | B: 499, C: 490 | 1.928 | 0.260 | 71.44 | 10.409 |
| 3 | 2 | 500 | B: 499, C: 490 | 2.207 | 0.241 | 71.14 | 10.434 |
| 3 | 3 | 500 | B: 499, C: 490 | 2.331 | 0.288 | 71.50 | 10.419 |

All six runs passed the delivery threshold and teardown assertions. Average user-plus-system CPU was 1.86 seconds for two participants and 2.42 seconds for three. Mean peak RSS was 71.78 MiB and 71.36 MiB, respectively; the similar peaks mostly reflect the fixed test runtime and are not evidence that room size has no memory cost.

## Difficulties and Limitations

- The server only registers a remote publication after RTP causes `on_track`; waiting for publication before sending RTP timed out. The benchmark now uses the same continuous warm-up approach as the existing relay test and excludes those packets from the measured counts.
- This is an in-process `WebRtcAgent` loopback test, not a measurement of the Axum/WebSocket server process, browser peers, or a deployed SFU. It uses one publisher only; it does not represent every participant publishing audio and video simultaneously.
- The workload is short and modest, with one fixed packet size/rate and only three repetitions. Host contention, production builds, longer calls, multiple tracks, more rooms, aggregate egress bandwidth, and network impairment were not tested.
- Peak RSS includes the Rust test harness and WebRTC runtime and is a process lifetime high-water mark. It cannot isolate per-peer or per-track memory.
- Repeated runs verify registry cleanup, but do not prove that all operating-system resources are reclaimed over long-lived/repeated room churn or that renegotiation remains healthy at scale.

## Provisional Capacity Recommendation

Keep `MAX_PEERS_PER_ROOM` at 2. Three peers delivered this specific single-publisher workload, but the small loopback sample is not sufficient to raise the production limit or claim a supported three-person meeting capacity. The configured three-peer admission test and this RTP benchmark are functional checks, not production capacity evidence.

## Problems to Handle Next

- Benchmark the actual server process and full signaling path in an optimized build, with repeatable CPU affinity/load conditions and longer steady-state runs.
- Exercise all participants publishing representative audio/video tracks; report per-peer receive loss, bitrate, CPU, peak and steady-state memory, and setup/renegotiation latency.
- Increase participant and room counts progressively, include packet loss/jitter and disconnect/reconnect churn, and investigate bandwidth and CPU saturation points.
- Verify repeated room teardown under load for task, socket, publication, and memory growth; add an explicit upper-bound assertion for cleanup time.
- Use those results to define supported room capacity and operational CPU, memory, bandwidth, and media-quality headroom before changing the default limit.

# Progress Report: November 9, 2026

## Meeting Creation and Guest Join

Implemented the first-pass guest-meeting flow for the Monday sprint. The project now has a room-creation API, guest ticket issuance, secure random room and ticket material, and a minimal create/join browser page that no longer relies on the fixed `demo-room` flow.

- Added `POST /api/meeting` to create a meeting and return a room ID plus host ticket.
- Added `POST /api/meeting/:room/ticket` to request a short-lived guest join ticket.
- Added secure random room IDs and ticket nonces via `getrandom` in `src/app.rs`.
- Enforced ticket validation during `WebSocket` admission when a `ticket` query parameter is supplied.
- Replaced the embedded page in `src/ui.rs` with a create/join form that accepts a display name and either a room code or invite URL.

## Status

The Monday foundation is now in place. The remaining work is not a blank slate anymore; the next sprint step is to tighten authorization, waiting-room semantics, and room-scoped roles for Tuesday.

## Validation

- `cargo test --quiet` passed: 15 passed, 0 failed, 2 ignored.
- The existing room-capacity, message-size, and WebSocket regression tests remain green after the meeting/ticket changes.

## Next Action

Move to the Tuesday authorization and lobby work: server-authoritative roles, host admission, origin checks, and room-scoped moderation before the full meeting flow is considered complete.

# Progress Report: November 10, 2026

## Authorization and socket admission

Completed the first pass of the Tuesday authorization work for the guest-meeting flow.

- Added a server-side ticket resolver that binds the socket to the canonical host/guest role from the meeting record instead of trusting client-provided identity or role fields.
- Enforced single-use guest ticket consumption, expiry checks, and lobby rejection when a meeting is in the waiting-room state.
- Added a production-safe origin allowlist check for browser-facing WebSocket upgrades using `WEBRTC_ALLOWED_ORIGINS` / `ALLOWED_ORIGINS`, with startup failure in `APP_ENV=production` if the allowlist is missing.
- Kept the existing room admission behavior intact while ensuring the socket admission path rejects unauthorized ticket or origin attempts before the connection is upgraded.

## Validation

The November 10 changes were verified with the project’s standard Rust checks:

- `cargo fmt` passed.
- `cargo check` passed.
- `cargo test --quiet` passed: 16 passed, 0 failed, 2 ignored.

## Remaining Tuesday work

The core November 10 meeting-control loop is now in place. The remaining non-core follow-up is the queue/rate-limit hardening pass:

- bounded queue / slow-client policy for per-socket outbound traffic
- connection and signaling rate limits
- payload-size bounds for participant names and state updates

The auth, origin, lobby, roster, and host-control slices are implemented and validated; the remaining work is operational hardening rather than missing core meeting authorization behavior.

# Progress Report: November 10 Hardening Pass

## Bounded outbound traffic and payload safety

Completed the Tuesday hardening pass for per-socket backpressure, signaling throttling, and payload validation.

- Replaced the per-socket outbound fan-out path with a bounded `tokio::sync::mpsc::channel` so a slow client gets dropped without unbounded memory growth.
- Retained the WebRTC agent's internal signaling channels as-is while forwarding server-generated WebRTC events through the bounded socket queue to preserve the room-flow semantics without a server-side queue blow-up.
- Added display-name and signal-state validators that cap participant names and state payload bytes before the state is accepted.
- Added a simple rate-limiter utility that tracks event bursts in a rolling window and rejects excess signaling spam.

## Validation

Verified with the project’s standard checks:

- `cargo fmt && cargo check && cargo test --quiet` passed.
- Test result: 18 passed, 0 failed, 2 ignored.

## Status

The November 10 operational hardening tasks are complete and recorded in the codebase. The remaining open work is the broader Wednesday/Thursday meeting UX and media-system milestones, not the socket safety layer or the authorization flow that was previously blocked.

# Progress Report: November 11, 2026

## Meeting Room Experience

Replaced the create/join-only page with an embedded meeting room view in `src/ui.rs`. It includes a room title and room-only copy invite, live participant roster, local preview, remote media tiles, mic/camera state, connection status, grid/speaker layout controls, pin buttons, and a compact control bar. The layout adapts to narrow viewports, controls use native buttons with accessible names and pressed states, and status changes use a polite live region.

The browser now requests audio and video independently, allowing either capture to fail without blocking the other. It groups relayed tracks by the source peer encoded in the relay stream ID, retains a peer's stream as individual tracks arrive, removes media on track/peer teardown, and analyzes remote audio levels for speaker highlighting. Host credentials are kept in session storage for host refresh; guest links contain only the room ID, and guest tickets are requested for the WebSocket connection.

The first live smoke test exposed a deadlock in `src/app.rs`: participant state handling held the meeting mutex while calling the roster broadcaster, which reacquired that mutex. The state lock is now released before broadcasting.

Automated three-peer Chromium coverage then exposed an SFU replay gap: a late joiner's initial offer had only one audio and one video section, so replaying publications from multiple existing peers left some tracks without negotiated sections. The server now schedules a follow-up offer when replay adds tracks and queues it after the initial answer, preserving message order. The browser serializes inbound signaling messages so an answer is applied before the queued offer. The suite confirms every participant receives both audio and video from both remote peers.

## Validation

- `cargo fmt --check` passed after the final Rust code changes.
- `cargo check` passed after the final Rust code changes.
- `cargo test --quiet` passed after the replay renegotiation fix: 18 passed, 0 failed, 2 ignored.
- `npm run test:browser -- --reporter=list` passed: 4 browser tests, 0 failures. The tests use Chromium fake media devices and a server configured to allow 3 participants.
- The three-participant RTP, roster, control, and leave test passed three additional consecutive focused runs.
- The embedded JavaScript parsed successfully with Node's `new Function` syntax check.
- Browser coverage verifies three participants receive audio/video from both remote peers; stable roster and tile cleanup on leave; local preview track preservation; active RTP bytes and track/sender identity across grid, speaker, and pin changes; keyboard operation of controls; microphone/camera roster updates; room-only copied invites; independent audio/video permission denial; and mobile-width overflow and visible keyboard focus.
- `npm ci` and `npx playwright install chromium` install the browser-test prerequisites; `npm run test:browser` runs the suite.

## Remaining Verification and Limitations

- The current default room capacity remains 2. The browser suite uses 3 only for verification; this is not representative capacity/load evidence and does not change the default.
- Speaker highlighting uses browser-side audio-level analysis; this is a presentation hint, not a server-verified speaking indicator.
- Browser media remains dependent on device permissions and the configured public STUN server. TURN and reconnect recovery are still future work.

# Capacity Default Update: November 11, 2026

Changed `MAX_PEERS_PER_ROOM` from 2 to 4 at the user's request. The default-capacity admission test now verifies four peers are admitted and a fifth is rejected; the explicit `MAX_PEERS_PER_ROOM` override remains available.

This changes the configured admission limit only. It does not establish four participants as a load-validated supported capacity: existing RTP measurements use a short single-publisher workload, and the optimized four-participant, all-publishers acceptance workload remains outstanding. The default should be revisited if that acceptance workload fails.

Validation after the change: `cargo fmt --check`, `cargo check`, and `cargo test --quiet` passed (18 passed, 0 failed, 2 ignored). `npm run test:browser -- --reporter=list` also passed all 4 Chromium tests with the suite's explicit capacity override of 3. The default-admission test verifies four peers are admitted and a fifth is rejected.
