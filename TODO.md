# Development TODO

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
