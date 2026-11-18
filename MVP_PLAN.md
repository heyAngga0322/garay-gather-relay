# MVP Work Plan

## Goal

Reach a credible MVP-level product from the current verified guest-meeting slice. The objective is to close the remaining reliability, capacity, and production-readiness gaps so the app is usable, testable, and defensible as a small-group WebRTC meeting product rather than a demo prototype.

## Current status

The project is in a strong smoke-test state:

- Rust checks pass
- Browser end-to-end guest flow passes in the current workspace
- Screen sharing, reconnection, device switching, and meeting-room UX are implemented
- The remaining gaps are not feature absence alone; they are capacity validation, production readiness, and operational hardening

## Success definition

The product may be called MVP-ready only when the following are true:

- A fresh checkout can run the app with clear setup steps
- The guest-meeting flow works end-to-end in browser automation
- A representative multi-participant workload passes under documented conditions
- The default supported room capacity is validated by evidence, not assumption
- Security, deployment, and cleanup boundaries are documented and reviewed
- Remaining known limitations are explicitly called out instead of hidden

## Workstreams

### 1) Capacity and performance validation

Priority: highest

Objectives:
- Validate the default room capacity of 4 with real multi-participant evidence
- Run an optimized four-participant all-publishers workload
- Measure CPU, memory, packet delivery, negotiation time, and cleanup behavior
- Identify and fix or document any relay and teardown failures

Tasks:
- Create an optimized server build test harness for 4 participants publishing audio/video
- Run repeated four-participant workloads with fixed durations and packet rates
- Record CPU, memory, delivery, and cleanup results in PROGRESS.md
- Fail the default capacity if results are unstable or cleanup leaks state
- Decide whether 4 is a supported capacity or whether the default must be reduced

Definition of done:
- A documented multi-participant workload passes consistently
- The room default is backed by measured evidence
- No peer, publication, relay, task, or socket state remains after shutdown

### 2) Production readiness and deployment hardening

Priority: high

Objectives:
- Remove the gap between local demo readiness and production-grade deployment assumptions
- Make the operational and security story credible for a real MVP deployment

Tasks:
- Add secure deployment guidance for TLS and reverse-proxy setup
- Document TURN/STUN configuration for restrictive networks
- Add readiness/liveness checks and startup validation
- Add metrics and structured logging for rooms, peers, tracks, and relay state
- Create graceful shutdown and connection-draining behavior
- Review and document abuse limits, origin enforcement, and auth boundaries

Definition of done:
- Setup and deployment steps are documented for a fresh environment
- Security and networking assumptions are explicit
- Operational diagnostics are available when the service is running

### 3) Media reliability and recovery verification

Priority: high

Objectives:
- Guarantee that reconnects, ICE restarts, and device changes do not corrupt room state
- Make recovery behavior repeatable across real failure scenarios

Tasks:
- Exercise reconnect and recovery under browser automation repeatedly
- Validate ICE failure, retry exhaustion, and leave/rejoin states
- Verify device removal/replacement and permission revocation behavior
- Ensure screenshare end-to-end cleanup is deterministic

Definition of done:
- Recovery paths behave consistently without duplicate media tiles or stuck peer state
- Failure modes present clear user-facing recovery actions

### 4) Documentation and product clarity

Priority: medium

Objectives:
- Ensure the project documentation matches actual architecture and risk
- Prevent false claims about security, capacity, or completion

Tasks:
- Update CLAUDE.md with the actual media-routing and security design
- Keep PROGRESS.md current with commands, outcomes, and known limitations
- Keep TODO.md aligned with the actual status and evidence
- Separate verified MVP behavior from unverified production claims

Definition of done:
- The docs reflect the true architecture and known remaining risks
- No unchecked claims are presented as production evidence

### 5) Product polish and UX confidence

Priority: medium

Objectives:
- Make the meeting experience feel like a product, not a prototype
- Improve clarity for joining, permissions, network state, and recovery

Tasks:
- Ensure status messaging is unambiguous for reconnect, failure, and leave states
- Review keyboard and screen-reader behavior on critical controls
- Tighten copy for invites, meeting state, and permission failures
- Confirm no stale UI state remains after reconnect or peer teardown

Definition of done:
- Controls remain understandable under failures and permission denials
- The interface communicates product-state transitions clearly

## Timeline

### Phase 1: Reliability and capacity proof
- Run optimized multi-peer workload
- Verify room capacity under evidence
- Fix relay or teardown issues if uncovered

### Phase 2: Operational readiness
- Add deployment guidance and startup validation
- Add metrics, logging, readiness, and shutdown behavior

### Phase 3: Documentation and launch gate
- Update architecture documentation
- Reconcile all checklists and progress reports
- Decide whether the project is MVP-ready or must remain a verified prototype

## Exit gate for MVP claim

Only mark the product as MVP-level after all of the following are true:

- Multi-participant workload evidence exists at the configured default capacity
- Browser end-to-end flow is consistently passing with real device scenarios
- Recovery and teardown behavior are proven under failure conditions
- Deployment and security assumptions are documented in plain language
- No major known risks are hidden behind the term "MVP"

## Immediate next actions

1. Run the optimized four-participant workload and record the actual results
2. Decide whether the default room capacity remains 4 or must be lowered
3. Document the real operational and deployment assumptions
4. Update all project docs to match the verified state
5. Only then classify the product as MVP-level
