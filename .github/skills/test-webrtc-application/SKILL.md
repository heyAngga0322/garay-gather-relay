---
name: test-webrtc-application
description: 'Test and troubleshoot this Rust and browser WebRTC meeting app. Use when running Rust checks, Playwright meeting flows, validating signaling or media changes, or diagnosing browser test failures.'
argument-hint: 'What changed or which WebRTC behavior should be tested?'
---

# Test the WebRTC Application

## Purpose

Verify changes to Garay at the right layer: Rust tests for server and relay behavior, and Playwright for browser meeting workflows. Keep test claims aligned with what the harness actually exercises.

## Workflow

1. Identify the behavior and owning layer before running tests:
   - Room creation, tickets, admission, routing, message validation, and cleanup: `src/app.rs`.
   - Signal payloads and serialization: `src/signaling.rs`.
   - SDP/ICE handling, peer lifecycle, and RTP relay: `src/webrtc.rs`.
   - Browser negotiation, controls, media UI, and recovery: `src/ui.rs` and `tests/browser/`.
2. Run the narrowest relevant check first. For Rust changes, use a focused test when practical, then run `cargo fmt --check`, `cargo check`, and `cargo test` for a Rust change set.
3. For browser-facing changes, run the Chromium end-to-end suite with `npm run test:browser -- --reporter=list`. The Playwright configuration starts the server with `MAX_PEERS_PER_ROOM=3`; the suite is sequential and uses fake camera/microphone inputs.
4. If browser dependencies are not installed, run `npm ci` and `npx playwright install chromium`, then retry the browser suite.
5. When a check fails, capture the failing test and assertion, inspect the owning implementation and related setup, make the smallest targeted fix, and rerun that same check before broadening validation.
6. After focused checks pass, run the remaining checks required by the change. Report the exact commands and distinguish passed checks from checks not run.

## Interpreting Results

- Rust tests cover server room/admission behavior, signaling models, WebRTC negotiation/relay, and cleanup. Use `cargo test -- --list` to inspect available tests before selecting a narrow test filter.
- Browser tests verify the implemented browser workflow in Chromium with simulated media and permissions. They do not validate physical camera/microphone devices, real-world NAT traversal, cross-browser behavior, or production network conditions.
- The Playwright room limit of three is test configuration, not evidence that the default four-participant capacity is supported. Capacity claims require a separately designed and documented workload.
- Resource-baseline or other ignored tests do not run in the default `cargo test` suite. Run them only when their documented prerequisites and methodology are understood; do not treat a short smoke test as capacity evidence.
- Playwright retains traces on failure. Inspect the trace and test output before attributing a failure to application logic; also check whether a reused server on port 3000 is running the expected build and configuration.
- Tests can establish regression coverage for modeled cases, but do not by themselves establish production readiness or security.

## Useful Commands

```sh
cargo fmt --check
cargo check
cargo test
npm run test:browser -- --reporter=list
```

For browser prerequisites:

```sh
npm ci
npx playwright install chromium
```

For local server health while investigating a browser run:

```sh
curl -s http://127.0.0.1:3000/health
```