# Garay Operations Runbook

This runbook covers day-to-day operations, troubleshooting, and incident response for the Garay WebRTC Signaling Server.

---

## Table of Contents

1. [Startup/Shutdown Procedures](#startupshutdown-procedures)
2. [Key Metrics & Alerting](#key-metrics--alerting)
3. [Troubleshooting Common Issues](#troubleshooting-common-issues)
4. [Log Analysis](#log-analysis)
5. [Capacity Planning](#capacity-planning)
6. [Incident Response](#incident-response)

---

## Startup/Shutdown Procedures

### Systemd (Production)

```bash
# Start
sudo systemctl start garay

# Stop (graceful - drains connections)
sudo systemctl stop garay

# Restart (graceful)
sudo systemctl restart garay

# Status
sudo systemctl status garay

# Logs
sudo journalctl -u garay -f
sudo journalctl -u garay --since "1 hour ago"

# Enable at boot
sudo systemctl enable garay
```

### Docker Compose (Development/Staging)

```bash
# Start
docker compose up -d

# Stop (graceful)
docker compose down

# Restart
docker compose restart

# Logs
docker compose logs -f garay

# With TURN
docker compose -f docker-compose.yml -f docker-compose.turn.yml up -d
```

### Manual (Development)

```bash
# With config.toml
cargo run

# With environment variables
GARAY__BIND_ADDRESS=0.0.0.0:3000 \
GARAY__LOG_FORMAT=text \
GARAY__LOG_LEVEL=debug \
cargo run
```

### Graceful Shutdown Behavior

Garay handles `SIGTERM` (systemd stop, docker stop) gracefully:
1. Stops accepting new WebSocket connections
2. Waits for in-flight signaling messages to complete (up to 30s)
3. Closes existing WebSocket connections with close frame
4. Tears down WebRTC peer connections and relay tasks
5. Exports final metrics
6. Exits

**Timeout**: 30 seconds (configurable via `GracefulShutdown` in Axum)

---

## Key Metrics & Alerting

All metrics exposed at `/metrics` (Prometheus format). Key metrics to monitor:

### WebSocket Connections

| Metric | Type | Description | Alert Threshold |
|--------|------|-------------|-----------------|
| `garay_ws_connections_active` | Gauge | Currently active WebSocket connections | > 80% of max capacity |
| `garay_ws_connections_total` | Counter | Total connections accepted | N/A |
| `garay_ws_connection_errors_total` | Counter | Connection errors (upgrade, origin, etc.) | Rate > 10/min |

### Signaling Messages

| Metric | Type | Description | Alert Threshold |
|--------|------|-------------|-----------------|
| `garay_signaling_messages_received_total` | Counter | Messages received by type | N/A |
| `garay_signaling_messages_sent_total` | Counter | Messages sent by type | N/A |
| `garay_signaling_message_size_bytes` | Histogram | Message payload size | p99 > 16KB |

### Room & Peer Lifecycle

| Metric | Type | Description | Alert Threshold |
|--------|------|-------------|-----------------|
| `garay_rooms_created_total` | Counter | Rooms created | N/A |
| `garay_peers_joined_total` | Counter | Peers joined rooms | N/A |
| `garay_peers_left_total` | Counter | Peers left (normal + error) | N/A |
| `garay_room_capacity_rejections_total` | Counter | Rejections due to room full | Rate > 5/min |

### WebRTC Negotiation

| Metric | Type | Description | Alert Threshold |
|--------|------|-------------|-----------------|
| `garay_webrtc_negotiation_duration_seconds` | Histogram | Offer/answer exchange duration | p99 > 10s |
| `garay_peer_connection_established_total` | Counter | Successful peer connections | N/A |
| `garay_peer_connection_failed_total` | Counter | Failed peer connections | Rate > 5/min |

### Media Relay

| Metric | Type | Description | Alert Threshold |
|--------|------|-------------|-----------------|
| `garay_tracks_published_total` | Counter | Media tracks published to relay | N/A |
| `garay_rtp_packets_forwarded_total` | Counter | RTP packets relayed | N/A |
| `garay_relay_errors_total` | Counter | Relay errors (send/recv failures) | Rate > 10/min |

### Resource Usage

| Metric | Type | Description | Alert Threshold |
|--------|------|-------------|-----------------|
| `garay_cpu_seconds_total` | Counter | Process CPU time (user+sys) | Rate > 80% of limit |
| `garay_memory_rss_bytes` | Gauge | Resident memory (RSS) | > 500MB |
| `garay_fd_count` | Gauge | Open file descriptors | > 80% of ulimit |

### HTTP Requests

| Metric | Type | Description | Alert Threshold |
|--------|------|-------------|-----------------|
| `garay_http_requests_total` | Counter | HTTP requests by path/status | N/A |
| `garay_http_request_duration_seconds` | Histogram | Request latency | p99 > 2s |

---

## Recommended Alert Rules (Prometheus Alertmanager)

```yaml
groups:
- name: garay
  rules:
  # High connection errors
  - alert: GarayHighConnectionErrors
    expr: rate(garay_ws_connection_errors_total[5m]) > 0.2
    for: 5m
    labels:
      severity: warning
    annotations:
      summary: "High WebSocket connection error rate"
      description: "{{ $value }} errors/sec"

  # Room capacity rejections
  - alert: GarayRoomCapacityRejections
    expr: rate(garay_room_capacity_rejections_total[5m]) > 0.1
    for: 5m
    labels:
      severity: warning
    annotations:
      summary: "Room capacity rejections increasing"
      description: "Users unable to join rooms"

  # WebRTC negotiation failures
  - alert: GarayWebRTCNegotiationFailures
    expr: rate(garay_peer_connection_failed_total[5m]) > 0.1
    for: 5m
    labels:
      severity: warning
    annotations:
      summary: "High peer connection failure rate"

  # Relay errors
  - alert: GarayRelayErrors
    expr: rate(garay_relay_errors_total[5m]) > 0.2
    for: 5m
    labels:
      severity: critical
    annotations:
      summary: "Media relay errors detected"
      description: "RTP packet forwarding failing"

  # High memory usage
  - alert: GarayHighMemoryUsage
    expr: garay_memory_rss_bytes > 500000000
    for: 10m
    labels:
      severity: warning
    annotations:
      summary: "Garay memory usage high"
      description: "RSS: {{ $value | humanize }}B"

  # High CPU usage
  - alert: GarayHighCPUUsage
    expr: rate(garay_cpu_seconds_total[5m]) > 0.8
    for: 10m
    labels:
      severity: warning
    annotations:
      summary: "Garay CPU usage high"
      description: "CPU: {{ $value }} cores"

  # Process down
  - alert: GarayProcessDown
    expr: up{job="garay"} == 0
    for: 1m
    labels:
      severity: critical
    annotations:
      summary: "Garay process not running"
```

---

## Troubleshooting Common Issues

### 1. ICE Connection Failures

**Symptoms**: Peers join room but no media flows; `iceConnectionState` stuck at `checking` or `failed`.

**Causes & Solutions**:

| Cause | Diagnosis | Solution |
|-------|-----------|----------|
| No TURN configured | Check `/api/ice-config` returns only STUN | Configure TURN in `config.toml` |
| TURN credentials wrong | Browser console: `ICE failed, see about:webrtc` | Verify username/credential match coturn |
| TURN port blocked | `turnutils_uclient` fails | Open firewall ports 3478/5349 UDP+TCP |
| Symmetric NAT | Works on same LAN, fails across NATs | TURN required (see above) |
| UDP blocked | TCP ICE candidates only | Ensure UDP 3478 open; try TURNS (TCP 5349) |

**Debug**:
```bash
# Test TURN from server
turnutils_uclient -u user -w pass turn.example.com

# Check browser ICE state
# Chrome: chrome://webrtc-internals
# Firefox: about:webrtc
```

### 2. Room Capacity Rejections

**Symptoms**: Users receive "Room is full" error; `garay_room_capacity_rejections_total` increases.

**Causes & Solutions**:

| Cause | Solution |
|-------|----------|
| Legitimate high demand | Increase `GARAY__MAX_PEERS_PER_ROOM` (max 16 recommended) |
| Stale peers not cleaned up | Check `garay_peers_left_total` matches joins; investigate cleanup |
| Reconnection storms | Implement client-side exponential backoff |

**Debug**:
```bash
# Check current rooms and peers
curl -s http://localhost:3000/metrics | grep garay_rooms
curl -s http://localhost:3000/metrics | grep garay_peers
```

### 3. WebSocket Connection Drops

**Symptoms**: Frequent reconnections; `garay_ws_connection_errors_total` spikes.

**Causes & Solutions**:

| Cause | Diagnosis | Solution |
|-------|-----------|----------|
| Origin mismatch | Logs show "Origin not allowed" | Set `WEBRTC_ALLOWED_ORIGINS` correctly |
| Proxy timeout | Drops at 60s idle | Increase proxy `proxy_read_timeout` for `/ws` |
| TLS cert issue | Browser blocks WSS | Fix cert chain; use valid CA |
| Server OOM | Logs show OOM kill | Increase memory limit; check for leaks |

### 4. Media Relay Errors

**Symptoms**: `garay_relay_errors_total` increases; users report frozen video/audio.

**Causes & Solutions**:

| Cause | Diagnosis | Solution |
|-------|-----------|----------|
| Peer connection closed | Logs show "peer not found" | Normal during leave; investigate if frequent |
| RTP send buffer full | High packet loss, congestion | Check network; reduce bitrate |
| Track negotiation mismatch | SDP mismatch in logs | Ensure browser sends consistent codecs |

### 5. High Latency / Poor Quality

**Symptoms**: Users report delay, jitter, robotic audio.

**Causes & Solutions**:

| Cause | Diagnosis | Solution |
|-------|-----------|----------|
| Server CPU saturated | `garay_cpu_seconds_total` high | Scale horizontally; reduce max peers |
| Network path issues | Traceroute shows high RTT | Deploy closer to users; use TURN |
| Browser encoding issues | High `nackCount`, `pliCount` in webrtc-internals | Simplify SDP; limit codecs |

---

## Log Analysis

### Structured JSON Log Fields

Garay logs JSON when `GARAY__LOG_FORMAT=json`:

```json
{
  "timestamp": "2026-11-20T10:30:45.123Z",
  "level": "INFO",
  "target": "garay::app",
  "message": "Peer joined room",
  "fields": {
    "room_id": "abc123",
    "peer_id": "peer_456",
    "role": "guest",
    "display_name": "Alice"
  }
}
```

### Key Fields for Correlation

| Field | Description | Use Case |
|-------|-------------|----------|
| `room_id` | Room identifier | Filter all logs for a meeting |
| `peer_id` | Server-assigned peer ID | Track single participant |
| `connection_id` | WebSocket connection ID | Correlate WS lifecycle |
| `trace_id` | (Future) Distributed trace ID | Cross-service tracing |

### Common Log Queries (jq)

```bash
# All events for a room
jq 'select(.fields.room_id == "abc123")' /var/log/garay/app.log

# Peer join/leave events
jq 'select(.message | test("joined|left"))' /var/log/garay/app.log

# Errors only
jq 'select(.level == "ERROR")' /var/log/garay/app.log

# WebSocket connection lifecycle
jq 'select(.fields.connection_id != null)' /var/log/garay/app.log

# Slow negotiations (>5s)
jq 'select(.fields.duration_ms > 5000)' /var/log/garay/app.log
```

### Log Rotation

Configure logrotate for systemd journal or file output:

```conf
# /etc/logrotate.d/garay
/var/log/garay/*.log {
    daily
    rotate 14
    compress
    delaycompress
    missingok
    notifempty
    create 0640 garay garay
    sharedscripts
    postrotate
        systemctl reload garay > /dev/null 2>&1 || true
    endscript
}
```

---

## Capacity Planning

### Baseline Resource Usage (4 peers, all publishing)

| Profile | CPU (user+sys) | Peak RSS | RTP Packets/10s |
|---------|----------------|----------|-----------------|
| Debug | ~31s | ~87 MB | ~21k per media type |
| Release | ~7.2s | ~87 MB | ~21k per media type |

### Scaling Guidelines

| Peers/Room | Max Rooms | Est. CPU (release) | Est. RSS | Recommended |
|------------|-----------|-------------------|----------|-------------|
| 4 | 25 | ~2 cores | ~2 GB | Single instance |
| 4 | 50 | ~4 cores | ~4 GB | 2 instances |
| 8 | 12 | ~3 cores | ~3 GB | 2 instances |
| 16 | 6 | ~4 cores | ~4 GB | 2 instances |

**Horizontal scaling**: Run multiple Garay instances behind load balancer with sticky sessions (by room_id) or shared Redis for room state (future work).

### Resource Limits

```yaml
# Kubernetes resource requests/limits
resources:
  requests:
    memory: "256Mi"
    cpu: "250m"
  limits:
    memory: "1Gi"
    cpu: "1000m"
```

---

## Incident Response

### Severity Levels

| Level | Definition | Response Time | Examples |
|-------|------------|---------------|----------|
| SEV-1 | Service down, data loss | Immediate | Process crashed, all rooms failing |
| SEV-2 | Major degradation | 15 min | High ICE failure rate, relay broken |
| SEV-3 | Minor issue | 1 hour | Elevated errors, slow negotiations |
| SEV-4 | Low priority | Next business day | Log noise, cosmetic issues |

### SEV-1 Runbook: Service Down

1. **Check process status**
   ```bash
   systemctl status garay
   # or
   docker compose ps
   ```

2. **Check logs for crash reason**
   ```bash
   journalctl -u garay --since "10 min ago" -p err
   ```

3. **Common causes**:
   - OOM kill: `dmesg | grep -i garay`
   - Port conflict: `ss -ltnp | grep 3000`
   - Config error: Check `config.toml` syntax
   - Dependency: TURN/reverse proxy down

4. **Restart**
   ```bash
   systemctl restart garay
   ```

5. **Verify recovery**
   ```bash
   curl -s http://localhost:3000/health
   curl -s http://localhost:3000/ready
   ```

### SEV-2 Runbook: High ICE Failures

1. **Check TURN connectivity**
   ```bash
   turnutils_uclient -u user -w pass turn.example.com
   ```

2. **Verify ICE config endpoint**
   ```bash
   curl -s http://localhost:3000/api/ice-config | jq .
   ```

3. **Check firewall / security groups**
   - UDP 3478, TCP 3478, UDP 5349, TCP 5349
   - Relay port range (49152-65535 UDP)

4. **Check coturn logs**
   ```bash
   docker compose logs coturn
   # or
   journalctl -u coturn
   ```

### SEV-2 Runbook: Media Relay Broken

1. **Check relay errors metric**
   ```bash
   curl -s http://localhost:3000/metrics | grep garay_relay_errors
   ```

2. **Check WebRTC agent state**
   - Logs show "relay task" errors
   - Peer connection cleanup issues

3. **Restart Garay** (drains connections gracefully)
   ```bash
   systemctl restart garay
   ```

---

## Maintenance Windows

### Routine Maintenance

| Task | Frequency | Procedure |
|------|-----------|-----------|
| Log rotation | Daily | Automatic via logrotate |
| Certificate renewal | 60 days | Automatic via certbot/cert-manager |
| TURN credential rotation | 90 days | Generate new secret, reload coturn & Garay |
| Security updates | Weekly | `apt update && apt upgrade` / rebuild Docker image |
| Metrics review | Weekly | Check Grafana dashboards for trends |

### Update Procedure

```bash
# 1. Build new image
docker compose build --no-cache

# 2. Rolling restart (zero-downtime with multiple replicas)
docker compose up -d --no-deps garay

# 3. Verify
curl -s http://localhost:3000/health
curl -s http://localhost:3000/metrics | head -20
```

---

## Contact & Escalation

| Role | Contact | When |
|------|---------|------|
| On-call | PagerDuty / Slack #garay-oncall | SEV-1, SEV-2 |
| Platform team | #platform-support | Infrastructure issues |
| Security | security@example.com | Suspected breach |

---

## Appendix: Useful Commands

```bash
# Quick health check
curl -s http://localhost:3000/health | jq .

# Readiness check
curl -s http://localhost:3000/ready | jq .

# All metrics
curl -s http://localhost:3000/metrics

# ICE config
curl -s http://localhost:3000/api/ice-config | jq .

# Create test meeting
curl -X POST http://localhost:3000/api/meeting \
  -H "Content-Type: application/json" \
  -d '{"display_name": "Test Host"}' | jq .

# Request guest ticket
curl -X POST http://localhost:3000/api/meeting/ROOM_ID/ticket \
  -H "Content-Type: application/json" \
  -d '{"display_name": "Test Guest"}' | jq .

# Systemd resource usage
systemd-cgtop -n 1 -b | grep garay

# Network connections
ss -ltnp | grep garay
ss -uanp | grep garay
```