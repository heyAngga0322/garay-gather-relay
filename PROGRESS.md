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
