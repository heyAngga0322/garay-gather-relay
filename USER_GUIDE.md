# Garay User Guide

**Garay (Gather Relay)** — A Rust WebRTC meeting server with SFU relay, guest-meeting flow, room admission, role-aware moderation, and browser-based media negotiation.

---

## Table of Contents

1. [Overview](#overview)
2. [Quick Start](#quick-start)
3. [Architecture](#architecture)
4. [Configuration](#configuration)
5. [API Reference](#api-reference)
6. [Deployment](#deployment)
7. [Browser Usage](#browser-usage)
8. [Operations](#operations)
9. [Troubleshooting](#troubleshooting)

---

## Overview

Garay is a small-group meeting server (max 4 peers by default) that provides:

- **Room creation & guest invitation** — Host creates a room, shares invite URL with guests
- **Ticket-based admission** — Single-use join tickets with host/guest roles
- **SFU media relay** — Server forwards RTP packets between participants (no media decoding)
- **Screen sharing, device switching, reconnect recovery** — Full meeting lifecycle support
- **Role-aware moderation** — Host can mute, admit, remove participants; room lock/lobby
- **Observability** — Structured JSON logging, Prometheus metrics, health/readiness/liveness endpoints

### Current Status

**Verified guest-meeting MVP** — Not production-hardened for public internet exposure.

| Feature | Status |
|---------|--------|
| Room creation & tickets | ✅ Working |
| WebRTC SFU relay | ✅ Validated (4 peers, 0% packet loss) |
| Browser UI | ✅ Embedded at `/` |
| Health/metrics endpoints | ✅ `/health`, `/ready`, `/live`, `/metrics` |
| TLS/HTTPS | ⚠️ Requires reverse proxy |
| TURN/STUN | ⚠️ Configurable, not production-ready |
| Multi-tenant persistence | ❌ In-memory only |

---

## Quick Start

### Prerequisites

- Rust 1.85+ (edition 2024)
- Tokio runtime (included)
- Optional: Docker, coturn for TURN

### Run Locally

```bash
# Clone and build
git clone <repo>
cd webrtc-axum

# Run server (listens on 0.0.0.0:3000)
cargo run

# Or run with custom config
GARAY__SERVER__BIND_ADDRESS=0.0.0.0:3000 \
GARAY__SERVER__MAX_PEERS_PER_ROOM=4 \
cargo run
```

### Health Check

```bash
curl -s http://127.0.0.1:3000/health
# {"service":"webrtc-signal-server","status":"ok"}
```

### Access Meeting UI

Open browser to: `http://localhost:3000/`

1. Enter display name
2. Click **"Create Room"** (host) or enter room code from invite
3. Share the invite URL with guests
4. Guests click link → enter name → join

---

## Architecture

### Two-Plane Design

```
┌─────────────────────────────────────────────────────────────┐
│                    SIGNALING PLANE (WS)                      │
│  Browser A ◄── WS /ws?room=R&ticket=T ──► Server ◄── WS ───►│
│  Browser B ◄── WS /ws?room=R&ticket=T ──► (app.rs) ──► WS ◄──│
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                     MEDIA PLANE (RTP)                        │
│  Browser A ── RTP ──► WebRtcAgent (SFU) ── RTP ──► Browser B │
│  Browser A ◄── RTP ── WebRtcAgent (webrtc.rs) ◄── RTP ◄── Browser B│
└─────────────────────────────────────────────────────────────┘
```

### Module Responsibilities

| Module | Purpose |
|--------|---------|
| `src/main.rs` | Entry point, config loading, logging, metrics, server startup |
| `src/app.rs` | Meeting creation, tickets, room membership, WebSocket lifecycle, routing |
| `src/signaling.rs` | Wire format (SignalMessage), validation, room IDs, ICE candidates |
| `src/ui.rs` | Embedded browser UI (HTML/JS), create/join, media negotiation, recovery |
| `src/webrtc.rs` | SFU relay (WebRtcAgent), peer connections, SDP/ICE, track fanout, RTP forwarding |
| `src/config.rs` | Configuration via TOML + `GARAY__` env vars |
| `src/metrics.rs` | Prometheus metrics definitions |

### Meeting Lifecycle

1. **Create** — Browser opens `/`, host enters name → `POST /api/meeting` → returns `host_ticket` + `invite_url`
2. **Invite** — Host shares `invite_url` (contains room + ticket) or guest requests ticket via `POST /api/meeting/:room/ticket`
3. **Join** — Browser connects `GET /ws?room=<room>&ticket=<ticket>` → server validates ticket → assigns `peer_id` → sends `welcome` with roster
4. **Negotiate** — Each peer sends `offer` to **server** (not other peers) → server responds with `answer` → ICE candidates exchanged with server
5. **Media** — Local tracks published to SFU → relayed to all other room members
6. **Control** — Host actions (mute, admit, remove, lock) via signaling messages
7. **Leave** — Disconnect → server cleans up peer state, relays, sends `peer-left` to room

---

## Configuration

Configuration priority: **Environment variables** > **config.toml** > **Defaults**

### Environment Variables (`GARAY__` prefix)

```bash
# Server
GARAY__SERVER__BIND_ADDRESS=0.0.0.0:3000
GARAY__SERVER__MAX_PEERS_PER_ROOM=4
GARAY__SERVER__LOG_LEVEL=info
GARAY__SERVER__LOG_FORMAT=json        # json or text

# TLS (for direct TLS; prefer reverse proxy)
GARAY__TLS__ENABLED=false
GARAY__TLS__CERT_PATH=/path/cert.pem
GARAY__TLS__KEY_PATH=/path/key.pem

# TURN/STUN
GARAY__TURN__ENABLED=true
GARAY__TURN__SERVER_URLS=["turn:turn.example.com:3478?transport=udp"]
GARAY__TURN__USERNAME=user
GARAY__TURN__CREDENTIAL=pass

# Allowed Origins (required in production)
GARAY__ALLOWED_ORIGINS=https://meet.example.com,https://app.example.com

# Environment
APP_ENV=production                  # development | production
```

### config.toml Example

See `config.toml.example` for fully documented example:

```toml
[server]
bind_address = "0.0.0.0:3000"
max_peers_per_room = 4
log_level = "info"
log_format = "json"

[tls]
enabled = false
cert_path = ""
key_path = ""

[turn]
enabled = true
server_urls = ["stun:stun.l.google.com:19302"]
username = ""
credential = ""
```

### Load Config in Code

```rust
use config::ServerConfig;
let config = ServerConfig::load()?;
```

---

## API Reference

### Base URL

```
http://localhost:3000
```

### REST Endpoints

#### Create Meeting (Host)

```http
POST /api/meeting
Content-Type: application/json

{"display_name": "Alice"}
```

**Response 200:**
```json
{
  "room_id": "AbC123-XyZ",
  "invite_url": "http://localhost:3000/?room=AbC123-XyZ&ticket=host:AbC123-XyZ:nonce",
  "host_ticket": "host:AbC123-XyZ:nonce"
}
```

#### Request Guest Ticket

```http
POST /api/meeting/{room}/ticket
Content-Type: application/json

{"display_name": "Bob"}
```

**Response 200:**
```json
{
  "room_id": "AbC123-XyZ",
  "ticket": "guest:AbC123-XyZ:nonce",
  "invite_url": "http://localhost:3000/?room=AbC123-XyZ&ticket=guest:AbC123-XyZ:nonce"
}
```

#### ICE Configuration

```http
GET /api/ice-config
```

**Response 200:**
```json
{
  "iceServers": [
    {"urls": ["stun:stun.l.google.com:19302"]},
    {"urls": ["turn:turn.example.com:3478?transport=udp"], "username": "user", "credential": "pass"}
  ]
}
```

#### Health / Readiness / Liveness

```http
GET /health    # {"status":"ok","service":"webrtc-signal-server"}
GET /ready     # {"status":"ready","service":"webrtc-signal-server"}
GET /live      # {"status":"alive","service":"webrtc-signal-server"}
```

#### Metrics (Prometheus)

```http
GET /metrics
# Returns text/plain metrics: garay_* counters, gauges, histograms
```

### WebSocket Signaling

**Connect:**
```
GET /ws?room=<room_id>&ticket=<ticket>
Upgrade: websocket
```

**Message Format (`SignalMessage`):**

```json
{
  "type": "offer|answer|candidate|welcome|peer-joined|peer-left|roster|state|relay-error",
  "room": "room_id",
  "peer_id": "server-assigned-peer-id",
  "from": "sender-peer-id",
  "to": "target-peer-id-or-server-or-empty-for-broadcast",
  "display_name": "Alice",
  "role": "host|guest",
  "muted": false,
  "camera_on": true,
  "screen_share": false,
  "sdp": "v=0\\r\\no=- ...",
  "candidate": {"candidate": "...", "sdpMid": "0", "sdpMLineIndex": 0},
  "participants": [...]
}
```

**Key Rules:**
- **Server owns** `room`, `peer_id`, `from` — client values are **overwritten**
- **Offers/answers/ICE** are **targeted** (`to` = specific peer or `"server"`) — never broadcast
- **SFU architecture**: Peers send `offer` to `"server"`, receive `answer` from `"server"`

### Message Flow Example (SFU)

```
Host (peer-A)                          Server                          Guest (peer-B)
    │                                     │                                     │
    ├── WS connect (host ticket) ───────►│                                     │
    │◄──── welcome (peer-A, roster=[]) ───┤                                     │
    │                                     │                                     │
    │                                     │◄──── WS connect (guest ticket) ──────┤
    │                                     │──── welcome (peer-B, roster=[A]) ──►│
    │                                     │──── peer-joined (peer-B) ─────────►│
    │◄──── peer-joined (peer-B) ─────────┤                                     │
    │                                     │                                     │
    ├── offer (to="server") ────────────►│                                     │
    │◄──── answer (from="server") ───────┤                                     │
    │                                     │                                     │
    │                                     │◄──── offer (to="server") ──────────┤
    │                                     │──── answer (from="server") ────────►│
    │                                     │                                     │
    ├── candidate (to="server") ────────►│                                     │
    │◄──── candidate (from="server") ────┤                                     │
    │                                     │◄──── candidate (to="server") ──────┤
    │                                     │──── candidate (from="server") ─────►│
    │                                     │                                     │
    │         ✅ Media flows via SFU (RTP)                                     │
```

---

## Deployment

### Docker (Multi-Stage)

```dockerfile
# Dockerfile - Build stage
FROM rust:1.85-slim AS builder
WORKDIR /app
COPY . .
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*
RUN useradd -r -s /bin/false appuser
WORKDIR /app
COPY --from=builder /app/target/release/garay .
USER appuser
EXPOSE 3000
HEALTHCHECK CMD curl -f http://localhost:3000/health || exit 1
CMD ["./garay"]
```

**Build & Run:**
```bash
docker build -t garay .
docker run -p 3000:3000 \
  -e GARAY__SERVER__BIND_ADDRESS=0.0.0.0:3000 \
  -e GARAY__ALLOWED_ORIGINS=https://your-domain.com \
  garay
```

### Docker Compose (Dev Stack)

```yaml
# docker-compose.yml
version: '3.8'
services:
  garay:
    build: .
    ports:
      - "3000:3000"
    environment:
      - GARAY__SERVER__BIND_ADDRESS=0.0.0.0:3000
      - GARAY__ALLOWED_ORIGINS=http://localhost:3000
      - APP_ENV=development
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:3000/health"]
      interval: 30s
      timeout: 5s
      retries: 3

  # Optional: TURN server (coturn)
  turn:
    image: coturn/coturn:latest
    network_mode: host
    command: >
      -n -v
      --realm=garay.local
      --user=user:pass
      --lt-cred-mech
      --no-tls
      --no-dtls
    profiles: ["turn"]
```

```bash
# Run dev stack
docker compose up -d

# Run with TURN
docker compose --profile turn up -d
```

### Systemd Service

```ini
# /etc/systemd/system/garay.service
[Unit]
Description=Garay WebRTC Signaling Server
After=network.target

[Service]
Type=simple
User=garay
Group=garay
WorkingDirectory=/opt/garay
EnvironmentFile=/opt/garay/garay.env
ExecStart=/opt/garay/garay
Restart=on-failure
RestartSec=5
StandardOutput=journal
StandardError=journal
# Security hardening
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/opt/garay

[Install]
WantedBy=multi-user.target
```

```bash
# Environment file: /opt/garay/garay.env
GARAY__SERVER__BIND_ADDRESS=0.0.0.0:3000
GARAY__SERVER__MAX_PEERS_PER_ROOM=4
GARAY__SERVER__LOG_LEVEL=info
GARAY__SERVER__LOG_FORMAT=json
GARAY__ALLOWED_ORIGINS=https://meet.example.com
APP_ENV=production

# Enable service
sudo systemctl daemon-reload
sudo systemctl enable --now garay
sudo journalctl -u garay -f
```

### TLS Termination (Reverse Proxy)

**Garay speaks plain HTTP/WS. Terminate TLS at proxy.**

#### Nginx

```nginx
# /etc/nginx/sites-available/garay
server {
    listen 443 ssl http2;
    server_name meet.example.com;

    ssl_certificate /etc/letsencrypt/live/meet.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/meet.example.com/privkey.pem;

    # WebSocket support
    location /ws {
        proxy_pass http://127.0.0.1:3000;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_read_timeout 86400;
    }

    location / {
        proxy_pass http://127.0.0.1:3000;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}

server {
    listen 80;
    server_name meet.example.com;
    return 301 https://$server_name$request_uri;
}
```

#### Caddy (Auto HTTPS)

```caddy
# Caddyfile
meet.example.com {
    reverse_proxy localhost:3000 {
        header_up Host {host}
        header_up X-Real-IP {remote_host}
        header_up X-Forwarded-For {remote_host}
        header_up X-Forwarded-Proto {scheme}
    }
}
```

#### Traefik (Kubernetes Ingress)

```yaml
# IngressRoute (Traefik CRD)
apiVersion: traefik.io/v1alpha1
kind: IngressRoute
metadata:
  name: garay
spec:
  entryPoints:
    - websecure
  routes:
    - match: Host(`meet.example.com`)
      kind: Rule
      services:
        - name: garay
          port: 3000
  tls:
    certResolver: letsencrypt
```

---

## Browser Usage

### Embedded UI

Open `http://localhost:3000/` (or your domain) in a modern browser:
- Chrome/Edge 100+, Firefox 100+, Safari 15+

### Meeting Controls

| Control | Action |
|---------|--------|
| **Microphone** | Toggle mute/unmute |
| **Camera** | Toggle video on/off |
| **Screen Share** | Start/stop screen capture (replaces camera) |
| **Layout** | Grid / Speaker view |
| **Pin** | Pin a participant's video |
| **Reconnect** | Automatic on network interruption |
| **Device Select** | Change camera/microphone/speaker |

### Screen Sharing Flow

1. Click **"Share Screen"** → browser picker → select screen/window/tab
2. Camera feed replaced by screen capture
3. Click **"Stop Sharing"** → camera restored automatically

### Reconnect Recovery

- On disconnect: UI shows "Reconnecting..."
- Browser re-establishes WebSocket with same ticket (if valid) or new ticket
- Media renegotiation happens automatically
- Remote tracks reappear without user action

### URL Format

```
https://meet.example.com/?room=<room_id>&ticket=<ticket>
```

Share this URL with guests. Tickets are **single-use**.

---

## Operations

### Key Metrics (Prometheus)

| Metric | Type | Description |
|--------|------|-------------|
| `garay_rooms_created_total` | Counter | Total rooms created |
| `garay_peers_joined_total` | Counter | Peers joined (by room) |
| `garay_peers_left_total` | Counter | Peers left (by room) |
| `garay_tickets_issued_total` | Counter | Tickets issued (by role) |
| `garay_capacity_rejections_total` | Counter | Room full rejections |
| `garay_signaling_messages_received_total` | Counter | WS messages received (by type) |
| `garay_signaling_messages_sent_total` | Counter | WS messages sent (by type) |
| `garay_webrtc_negotiation_duration_seconds` | Histogram | Offer/answer round-trip |
| `garay_peer_connection_established_total` | Counter | Successful peer connections |
| `garay_peer_connection_failed_total` | Counter | Failed peer connections |
| `garay_tracks_published_total` | Counter | Media tracks published |
| `garay_rtp_packets_forwarded_total` | Counter | RTP packets relayed (by kind) |
| `garay_relay_errors_total` | Counter | Relay errors (by type) |
| `garay_http_requests_total` | Counter | HTTP requests (by method, path, status) |
| `garay_http_request_duration_seconds` | Histogram | HTTP latency |
| `garay_cpu_seconds_total` | Counter | Process CPU time (user+sys) |
| `garay_resident_memory_bytes` | Gauge | RSS memory |

### Alerting Thresholds (Suggested)

```yaml
# Prometheus alerting rules
groups:
- name: garay
  rules:
  - alert: GarayHighCapacityRejections
    expr: rate(garay_capacity_rejections_total[5m]) > 0.1
    for: 2m
    labels:
      severity: warning
    annotations:
      summary: "High room capacity rejections"

  - alert: GarayHighRelayErrors
    expr: rate(garay_relay_errors_total[5m]) > 1
    for: 1m
    labels:
      severity: critical
    annotations:
      summary: "High WebRTC relay error rate"

  - alert: GarayHighConnectionFailures
    expr: rate(garay_peer_connection_failed_total[5m]) > 0.05
    for: 2m
    labels:
      severity: warning
    annotations:
      summary: "High WebRTC connection failure rate"

  - alert: GarayHighMemoryUsage
    expr: garay_resident_memory_bytes > 500000000
    for: 5m
    labels:
      severity: warning
    annotations:
      summary: "Garay RSS > 500MB"
```

### Log Analysis

**Structured JSON logs (when `log_format=json`):**

```json
{
  "timestamp": "2026-11-20T10:30:45.123Z",
  "level": "INFO",
  "target": "garay::app",
  "fields": {
    "peer_id": "peer-abc123",
    "room_id": "Room-XYZ",
    "display_name": "Alice",
    "role": "host",
    "message": "Peer joined"
  }
}
```

**Key fields for correlation:**
- `peer_id` — Track single participant across logs
- `room_id` — Filter by meeting
- `trace_id` — (if enabled via `tower-http` TraceLayer) End-to-end request tracing

### Common Operational Tasks

| Task | Command |
|------|---------|
| View logs | `journalctl -u garay -f` |
| Check metrics | `curl -s localhost:3000/metrics \| grep garay_` |
| Health check | `curl -s localhost:3000/health` |
| Restart service | `sudo systemctl restart garay` |
| View config | `cat /opt/garay/garay.env` |
| Check connections | `ss -tlnp \| grep 3000` |

---

## Troubleshooting

### Connection Issues

| Symptom | Cause | Fix |
|---------|-------|-----|
| `403 Origin not allowed` | Missing `GARAY__ALLOWED_ORIGINS` | Set allowed origins in config/env |
| `401 Ticket rejected` | Ticket used/expired/invalid | Issue new guest ticket |
| `503 Room at capacity` | Room full (default 4) | Increase `max_peers_per_room` or wait |
| WebSocket fails to upgrade | Proxy not passing Upgrade header | Configure proxy for WebSocket (`proxy_http_version 1.1`) |

### Media Issues

| Symptom | Cause | Fix |
|---------|-------|-----|
| No video/audio | ICE failed (NAT) | Configure TURN server in `config.toml` |
| One-way media | Firewall blocking UDP | Open UDP ports 49152-65535 or use TURN/TCP |
| High packet loss | Network congestion | Check `garay_rtp_packets_forwarded_total` vs drops |
| Screen share fails | Browser permissions | Allow screen capture in browser settings |

### Debug Commands

```bash
# Test ICE config endpoint
curl http://localhost:3000/api/ice-config

# Check server logs for peer
journalctl -u garay -f | grep "peer-abc123"

# Capture metrics snapshot
curl -s localhost:3000/metrics > metrics-$(date +%s).txt

# Test WebSocket manually
wscat -c "ws://localhost:3000/ws?room=test&ticket=guest:test:nonce"
```

### Browser DevTools

1. Open DevTools → **Console** — signaling messages, errors
2. **Network** tab → filter `WS` — WebSocket frames
3. **Application** tab → **WebRTC Internals** (chrome://webrtc-internals) — ICE state, DTLS, SRTP, stats
4. **Firefox**: `about:webrtc` — detailed connection stats

---

## Development

### Run Tests

```bash
# Unit tests
cargo test --lib

# Integration tests (real WebSocket clients)
cargo test --test signaling_integration

# Four-peer acceptance test (60s)
cargo test four_peer_all_publishers_acceptance -- --nocapture

# Browser tests (Playwright)
npm ci
npx playwright install chromium
npm run test:browser -- --reporter=list
```

### Code Style

```bash
cargo fmt --check
cargo check
cargo clippy
```

### Fix libsimdjson (Playwright)

```bash
# If Playwright fails: "libsimdjson.so.36 not found"
./fix-libsimdjson.sh
# or manually:
sudo ln -sf /usr/lib/libsimdjson.so.37 /usr/lib/libsimdjson.so.36
```

---

## Security Notes

⚠️ **This is a prototype, not hardened for public internet.**

| Gap | Mitigation |
|-----|------------|
| No TLS | Use reverse proxy (nginx/Caddy/Traefik) |
| No authentication system | Tickets are short-lived meeting credentials only |
| In-memory state | Not multi-tenant; single process |
| Default STUN only | Configure TURN for restrictive NATs |
| No rate limiting on REST | Add proxy-level rate limiting |
| No audit logging | Add structured log shipping |

---

## License

MIT — See LICENSE file.

---

## Support

- **Issues**: GitHub Issues
- **Architecture**: See `CLAUDE.md` and `MVP_PLAN.md`
- **Operations**: See `OPERATIONS_RUNBOOK.md`
- **Deployment**: See `DEPLOYMENT.md`, `Dockerfile`, `docker-compose.yml`
- **TURN**: See `TURN_SETUP.md`
- **TLS**: See `TLS_PROXY.md`
- **Stress Testing**: See `STRESS_TESTING.md`