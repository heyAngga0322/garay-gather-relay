# TLS Reverse Proxy Configuration for Garay

**Production Recommendation**: Terminate TLS at the reverse proxy. Run Garay as plain HTTP internally.

```
Client (HTTPS/WSS) ──► Reverse Proxy (TLS Termination) ──► Garay (HTTP/WS)
     wss://meet.example.com                    http://127.0.0.1:3000
```

This approach:
- Simplifies Garay (no cert management in Rust)
- Enables centralized cert automation (Let's Encrypt, cert-manager)
- Allows Layer 7 routing, rate limiting, WAF
- Works with Kubernetes ingress, service mesh

---

## Option 1: Nginx

### Install

```bash
# Ubuntu/Debian
sudo apt install nginx certbot python3-certbot-nginx

# RHEL/Fedora
sudo dnf install nginx certbot python3-certbot-nginx
```

### Configuration: `/etc/nginx/sites-available/garay`

```nginx
# Garay WebRTC Signaling Server - Nginx Reverse Proxy
# Place at /etc/nginx/sites-available/garay
# Enable: ln -s /etc/nginx/sites-available/garay /etc/nginx/sites-enabled/

upstream garay_backend {
    server 127.0.0.1:3000;
    keepalive 32;
}

# HTTP -> HTTPS redirect
server {
    listen 80;
    listen [::]:80;
    server_name meet.example.com;

    # ACME challenge for Let's Encrypt
    location /.well-known/acme-challenge/ {
        root /var/www/certbot;
    }

    # Redirect all other traffic to HTTPS
    location / {
        return 301 https://$host$request_uri;
    }
}

# HTTPS server
server {
    listen 443 ssl http2;
    listen [::]:443 ssl http2;
    server_name meet.example.com;

    # SSL certificates (managed by certbot)
    ssl_certificate /etc/letsencrypt/live/meet.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/meet.example.com/privkey.pem;

    # Modern SSL config (Mozilla Intermediate)
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_ciphers ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-AES256-GCM-SHA384:ECDHE-RSA-AES256-GCM-SHA384:ECDHE-ECDSA-CHACHA20-POLY1305:ECDHE-RSA-CHACHA20-POLY1305:DHE-RSA-AES128-GCM-SHA256:DHE-RSA-AES256-GCM-SHA384;
    ssl_prefer_server_ciphers off;
    ssl_session_timeout 10m;
    ssl_session_cache shared:SSL:10m;
    ssl_session_tickets off;

    # OCSP Stapling
    ssl_stapling on;
    ssl_stapling_verify on;
    resolver 1.1.1.1 8.8.8.8 valid=300s;
    resolver_timeout 5s;

    # Security headers
    add_header Strict-Transport-Security "max-age=31536000" always;
    add_header X-Frame-Options DENY;
    add_header X-Content-Type-Options nosniff;
    add_header Referrer-Policy strict-origin-when-cross-origin;

    # WebSocket proxying for /ws
    location /ws {
        proxy_pass http://garay_backend;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;

        # WebSocket timeouts
        proxy_read_timeout 86400;
        proxy_send_timeout 86400;

        # Buffer settings for WebSocket
        proxy_buffering off;
    }

    # HTTP API and UI
    location / {
        proxy_pass http://garay_backend;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;

        # Timeouts
        proxy_connect_timeout 10s;
        proxy_read_timeout 60s;
        proxy_send_timeout 60s;
    }

    # Health checks (no auth, minimal logging)
    location /health {
        proxy_pass http://garay_backend;
        access_log off;
    }

    location /ready {
        proxy_pass http://garay_backend;
        access_log off;
    }

    location /live {
        proxy_pass http://garay_backend;
        access_log off;
    }

    # Metrics (restrict in production)
    location /metrics {
        proxy_pass http://garay_backend;
        allow 10.0.0.0/8;   # Private networks
        allow 172.16.0.0/12;
        allow 192.168.0.0/16;
        deny all;
    }
}
```

### Obtain certificates with Certbot

```bash
# Test configuration
sudo nginx -t

# Obtain certificate (first time)
sudo certbot --nginx -d meet.example.com

# Or standalone if nginx not running yet
sudo certbot certonly --standalone -d meet.example.com

# Auto-renewal (certbot installs systemd timer)
sudo systemctl enable certbot.timer
sudo systemctl start certbot.timer
```

### Reload Nginx

```bash
sudo nginx -t && sudo systemctl reload nginx
```

---

## Option 2: Caddy

Caddy provides automatic HTTPS via Let's Encrypt with minimal configuration.

### Install

```bash
# Ubuntu/Debian
sudo apt install -y debian-keyring debian-archive-keyring apt-transport-https curl
curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' | sudo gpg --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' | sudo tee /etc/apt/sources.list.d/caddy-stable.list
sudo apt update && sudo apt install caddy

# Or download binary: https://caddyserver.com/download
```

### Configuration: `/etc/caddy/Caddyfile`

```caddy
# Garay WebRTC Signaling Server - Caddy Reverse Proxy
# Place at /etc/caddy/Caddyfile

meet.example.com {
    # Automatic HTTPS via Let's Encrypt
    # Email for expiry notifications (optional)
    # email admin@example.com

    # Security headers
    header {
        Strict-Transport-Security "max-age=31536000"
        X-Frame-Options "DENY"
        X-Content-Type-Options "nosniff"
        Referrer-Policy "strict-origin-when-cross-origin"
    }

    # WebSocket proxy for /ws
    @websocket {
        path /ws
        header Connection *Upgrade*
        header Upgrade websocket
    }
    reverse_proxy @websocket 127.0.0.1:3000 {
        transport http {
            # WebSocket support
            read_buffer 4096
        }
        # Long timeouts for WebSocket
        transport http {
            read_timeout 24h
            write_timeout 24h
        }
    }

    # HTTP API and UI
    reverse_proxy 127.0.0.1:3000 {
        # Health checks
        health_uri /health
        health_interval 10s
        health_timeout 5s
    }

    # Metrics (restrict access)
    @metrics path /metrics
    reverse_proxy @metrics 127.0.0.1:3000 {
        # Restrict to private networks
        # Use Caddy's remote_ip matcher or auth
    }
}
```

### Run Caddy

```bash
# Validate config
caddy validate --config /etc/caddy/Caddyfile

# Run as service
sudo systemctl enable caddy
sudo systemctl start caddy

# Or run directly
caddy run --config /etc/caddy/Caddyfile
```

---

## Option 3: Traefik (Kubernetes Ingress)

For Kubernetes deployments, use Traefik as ingress controller.

### IngressRoute: `garay-ingress.yaml`

```yaml
apiVersion: traefik.io/v1alpha1
kind: IngressRoute
metadata:
  name: garay
  namespace: garay
spec:
  entryPoints:
    - websecure
  routes:
    # WebSocket route (must match before HTTP)
    - match: Host(`meet.example.com`) && PathPrefix(`/ws`)
      kind: Rule
      services:
        - name: garay
          port: 3000
          scheme: http
      middlewares:
        - name: websocket-headers
    # HTTP API and UI
    - match: Host(`meet.example.com`)
      kind: Rule
      services:
        - name: garay
          port: 3000
          scheme: http
      middlewares:
        - name: security-headers
        - name: garay-rate-limit
  tls:
    certResolver: letsencrypt

---
# Middleware for WebSocket headers
apiVersion: traefik.io/v1alpha1
kind: Middleware
metadata:
  name: websocket-headers
  namespace: garay
spec:
  headers:
    customRequestHeaders:
      X-Forwarded-Proto: "https"

---
# Security headers middleware
apiVersion: traefik.io/v1alpha1
kind: Middleware
metadata:
  name: security-headers
  namespace: garay
spec:
  headers:
    sslRedirect: true
    stsSeconds: 31536000
    stsIncludeSubdomains: true
    stsPreload: true
    frameDeny: true
    contentTypeNosniff: true
    referrerPolicy: "strict-origin-when-cross-origin"

---
# Rate limiting middleware
apiVersion: traefik.io/v1alpha1
kind: Middleware
metadata:
  name: garay-rate-limit
  namespace: garay
spec:
  rateLimit:
    average: 100
    burst: 50
    sourceCriterion:
      ipStrategy:
        depth: 0
```

### Service: `garay-service.yaml`

```yaml
apiVersion: v1
kind: Service
metadata:
  name: garay
  namespace: garay
spec:
  selector:
    app: garay
  ports:
    - port: 3000
      targetPort: 3000
      name: http
```

### Deployment: `garay-deployment.yaml`

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: garay
  namespace: garay
spec:
  replicas: 2
  selector:
    matchLabels:
      app: garay
  template:
    metadata:
      labels:
        app: garay
    spec:
      containers:
        - name: garay
          image: your-registry/garay:latest
          ports:
            - containerPort: 3000
          env:
            - name: GARAY__BIND_ADDRESS
              value: "0.0.0.0:3000"
            - name: GARAY__LOG_FORMAT
              value: "json"
            - name: GARAY__LOG_LEVEL
              value: "info"
            - name: GARAY__MAX_PEERS_PER_ROOM
              value: "4"
            - name: WEBRTC_ALLOWED_ORIGINS
              value: "https://meet.example.com"
            - name: APP_ENV
              value: "production"
          envFrom:
            - secretRef:
                name: garay-secrets  # Contains TURN credentials
          resources:
            requests:
              memory: "128Mi"
              cpu: "100m"
            limits:
              memory: "512Mi"
              cpu: "500m"
          livenessProbe:
            httpGet:
              path: /live
              port: 3000
            initialDelaySeconds: 10
            periodSeconds: 30
          readinessProbe:
            httpGet:
              path: /ready
              port: 3000
            initialDelaySeconds: 5
            periodSeconds: 10
```

---

## Garay Configuration for Reverse Proxy

When using a reverse proxy, configure Garay for plain HTTP:

### config.toml

```toml
[server]
bind_address = "127.0.0.1:3000"  # Bind to localhost only
max_peers_per_room = 4
log_format = "json"
log_level = "info"

[server.tls]
tls_enabled = false  # TLS terminated at proxy

# TURN config (if using TURN)
[server.turn]
urls = [
  "stun:stun.l.google.com:19302",
  "turn:turn.example.com:3478?transport=udp",
  "turns:turn.example.com:5349?transport=tcp"
]
username = "garay-user"
credential = "your-turn-password"
```

### Environment variables

```bash
GARAY__BIND_ADDRESS=127.0.0.1:3000
GARAY__TLS_ENABLED=false
GARAY__MAX_PEERS_PER_ROOM=4
GARAY__LOG_FORMAT=json
GARAY__LOG_LEVEL=info
WEBRTC_ALLOWED_ORIGINS=https://meet.example.com
APP_ENV=production
```

---

## Security Checklist

- [ ] TLS terminated at reverse proxy (not in Garay)
- [ ] Garay binds to `127.0.0.1:3000` only (not `0.0.0.0`)
- [ ] `WEBRTC_ALLOWED_ORIGINS` set to your domain(s)
- [ ] `APP_ENV=production` (enforces origin check)
- [ ] Security headers: HSTS, X-Frame-Options, CSP
- [ ] Rate limiting on signaling endpoints
- [ ] Metrics endpoint restricted to monitoring network
- [ ] Health/ready/live endpoints accessible to load balancer
- [ ] Certificate auto-renewal configured (certbot, cert-manager)
- [ ] TURN credentials stored in secrets (not config files)