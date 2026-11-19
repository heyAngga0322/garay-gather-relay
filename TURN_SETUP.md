# TURN Server Setup Guide for Garay

This guide covers deploying a TURN/STUN server (coturn) for NAT traversal in restrictive network environments.

## Why TURN?

WebRTC requires ICE (Interactive Connectivity Establishment) to connect peers. In many networks:
- **STUN** works for public IPs and some NATs
- **TURN** is required for symmetric NATs, carrier-grade NAT (CGNAT), and restrictive firewalls

Without TURN, ~10-20% of connections fail in real-world deployments.

---

## Quick Start with Docker Compose

### 1. Create TURN configuration directory

```bash
mkdir -p coturn
```

### 2. Create `coturn/turnserver.conf`

```conf
# coturn/turnserver.conf - TURN/STUN server configuration
# Realm (your domain)
realm=garay.local

# Listening ports
listening-port=3478
tls-listening-port=5349

# External IP (required for cloud deployments)
# external-ip=YOUR_PUBLIC_IP

# Authentication
use-auth-secret
static-auth-secret=your-long-random-secret-here

# Or use user/password (less secure)
# user=garay-user:your-password

# Realm for TURNS
cert=/etc/coturn/certs/turn_server_cert.pem
pkey=/etc/coturn/certs/turn_server_pkey.pem

# Security
no-loopback-peers
no-multicast-peers

# Logging
log-file=stdout
verbose

# Performance
total-quota=100
bps-capacity=0
stale-nonce=600
```

### 3. Generate TLS certificates for TURNS (production)

```bash
mkdir -p coturn/certs

# Self-signed for development
openssl req -x509 -newkey rsa:2048 -keyout coturn/certs/turn_server_pkey.pem \
  -out coturn/certs/turn_server_cert.pem -days 365 -nodes \
  -subj "/CN=turn.garay.local"

# For production, use Let's Encrypt or your CA
```

### 4. Generate static auth secret

```bash
# Generate a secure random secret
openssl rand -base64 32
# Use this value for static-auth-secret in turnserver.conf
```

### 5. Update docker-compose.turn.yml with credentials

```yaml
# docker-compose.turn.yml
services:
  coturn:
    image: instrumentisto/coturn:latest
    container_name: garay-coturn
    restart: unless-stopped
    network_mode: host
    environment:
      - TURN_SERVER_REALM=garay.local
      - TURN_SERVER_USERNAME=garay-user
      - TURN_SERVER_PASSWORD=your-password
    volumes:
      - ./coturn/turnserver.conf:/etc/coturn/turnserver.conf:ro
      - ./coturn/certs:/etc/coturn/certs:ro
```

### 6. Start TURN server

```bash
docker compose -f docker-compose.yml -f docker-compose.turn.yml up -d
```

---

## Configure Garay to Use TURN

### Option 1: config.toml

```toml
[server.turn]
urls = [
  "stun:stun.l.google.com:19302",
  "turn:turn.garay.local:3478?transport=udp",
  "turns:turn.garay.local:5349?transport=tcp"
]
username = "garay-user"
credential = "your-turn-password"
```

### Option 2: Environment variables

```bash
export GARAY__TURN__URLS='["stun:stun.l.google.com:19302","turn:turn.garay.local:3478?transport=udp","turns:turn.garay.local:5349?transport=tcp"]'
export GARAY__TURN__USERNAME="garay-user"
export GARAY__TURN__CREDENTIAL="your-turn-password"
```

### Option 3: Static auth secret (preferred for coturn)

If using `static-auth-secret` in coturn, configure Garay with short-lived TURN credentials:

```toml
[server.turn]
urls = [
  "stun:stun.l.google.com:19302",
  "turn:turn.garay.local:3478?transport=udp",
  "turns:turn.garay.local:5349?transport=tcp"
]
# With static auth secret, credentials are time-limited HMAC
# Garay will need a small wrapper to generate ephemeral credentials
# See: https://tools.ietf.org/html/rfc5389#section-10.2
```

---

## Firewall Rules

Allow these ports for TURN traffic:

| Port | Protocol | Purpose |
|------|----------|---------|
| 3478 | UDP/TCP | TURN (plain) |
| 5349 | UDP/TCP | TURNS (TLS) |
| 49152-65535 | UDP | TURN relay ports (configure `min-port`/`max-port` in coturn) |

### UFW Example

```bash
sudo ufw allow 3478/udp
sudo ufw allow 3478/tcp
sudo ufw allow 5349/udp
sudo ufw allow 5349/tcp
sudo ufw allow 49152:65535/udp
```

### iptables Example

```bash
iptables -A INPUT -p udp --dport 3478 -j ACCEPT
iptables -A INPUT -p tcp --dport 3478 -j ACCEPT
iptables -A INPUT -p udp --dport 5349 -j ACCEPT
iptables -A INPUT -p tcp --dport 5349 -j ACCEPT
iptables -A INPUT -p udp --dport 49152:65535 -j ACCEPT
```

---

## Credential Rotation

For production, rotate TURN credentials periodically:

1. **Static auth secret**: Generate new secret, update coturn and Garay, reload both
2. **User/password**: Use `turnadmin` to add/remove users without restart

```bash
# Add user
turnadmin -a -u garay-user -r garay.local -p new-password

# Delete user
turnadmin -d -u garay-user -r garay.local
```

---

## Testing TURN Connectivity

### From browser console

```javascript
// Test ICE gathering
const pc = new RTCPeerConnection({
  iceServers: [
    { urls: "stun:stun.l.google.com:19302" },
    { urls: "turn:turn.garay.local:3478?transport=udp", username: "user", credential: "pass" }
  ]
});

pc.createDataChannel("test");
pc.createOffer().then(o => pc.setLocalDescription(o));

pc.onicecandidate = e => {
  if (e.candidate) console.log(e.candidate.candidate);
};
```

Look for `relay` type candidates in the output.

### Using `turnutils_uclient`

```bash
# Test TURN connectivity
turnutils_uclient -u garay-user -w your-password turn.garay.local

# Test TURNS
turnutils_uclient -u garay-user -w your-password -T turns:turn.garay.local:5349
```

---

## Monitoring coturn

### Health check

```bash
# Check if coturn is running
curl -f http://localhost:3478/health || echo "TURN not responding"

# Or use turnadmin
turnadmin -l
```

### Prometheus metrics (coturn 4.6+)

Enable in `turnserver.conf`:
```conf
prometheus-port=3479
prometheus-path=/metrics
```

Then scrape `http://turn-host:3479/metrics`.

---

## Production Checklist

- [ ] Use `static-auth-secret` with HMAC-based ephemeral credentials
- [ ] Configure TLS certificates from Let's Encrypt or internal CA
- [ ] Set `external-ip` in coturn for cloud deployments
- [ ] Restrict relay port range (`min-port`/`max-port`)
- [ ] Enable rate limiting (`total-quota`, `bps-capacity`)
- [ ] Monitor TURN allocation failures and relay usage
- [ ] Set up log aggregation for TURN logs
- [ ] Test from restricted networks (corporate, mobile, CGNAT)
- [ ] Document credential rotation procedure