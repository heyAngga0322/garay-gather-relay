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
