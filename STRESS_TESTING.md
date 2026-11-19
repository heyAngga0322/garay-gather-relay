# Network Impairment Stress Testing for Garay

This guide covers testing Garay under simulated adverse network conditions using Linux `tc` (traffic control) with `netem`.

---

## Prerequisites

```bash
# Install tc (usually in iproute2 package)
# Ubuntu/Debian:
sudo apt install iproute2

# Arch:
sudo pacman -S iproute2

# Verify
tc qdisc show
```

---

## Test Scenarios

| Scenario | Packet Loss | Latency (RTT) | Jitter | Reordering | Use Case |
|----------|-------------|---------------|--------|------------|----------|
| **Baseline** | 0% | 0ms | 0ms | 0% | Control |
| **Mild** | 1% | 50ms | 10ms | 0.5% | Typical WiFi |
| **Moderate** | 5% | 100ms | 30ms | 2% | Mobile 4G |
| **Severe** | 10% | 200ms | 50ms | 5% | Congested/International |
| **Extreme** | 20% | 500ms | 100ms | 10% | Satellite/Extreme |

---

## Applying Network Impairment

### On Linux (host running Garay or client)

```bash
# Apply to specific interface (e.g., eth0, wlan0, docker0)
INTERFACE="eth0"

# Clear existing rules
sudo tc qdisc del dev $INTERFACE root 2>/dev/null || true

# Baseline (no impairment)
sudo tc qdisc add dev $INTERFACE root handle 1: pfifo_fast

# Mild impairment (1% loss, 50ms RTT, 10ms jitter)
sudo tc qdisc add dev $INTERFACE root handle 1: netem \
    loss 1% \
    delay 25ms 10ms distribution normal \
    reorder 0.5% 50%

# Moderate impairment (5% loss, 100ms RTT, 30ms jitter)
sudo tc qdisc add dev $INTERFACE root handle 1: netem \
    loss 5% \
    delay 50ms 30ms distribution normal \
    reorder 2% 50%

# Severe impairment (10% loss, 200ms RTT, 50ms jitter)
sudo tc qdisc add dev $INTERFACE root handle 1: netem \
    loss 10% \
    delay 100ms 50ms distribution normal \
    reorder 5% 50%

# Extreme impairment (20% loss, 500ms RTT, 100ms jitter)
sudo tc qdisc add dev $INTERFACE root handle 1: netem \
    loss 20% \
    delay 250ms 100ms distribution normal \
    reorder 10% 50%
```

### For Docker Containers

Apply on host to `docker0` bridge or container's veth:

```bash
# Find container's veth interface
CONTAINER_ID=$(docker ps -qf name=garay)
VETH=$(docker exec $CONTAINER_ID ip link show | grep -oP 'veth\w+@' | head -1 | tr -d '@')
# Or apply on docker0 bridge
sudo tc qdisc add dev docker0 root netem loss 5% delay 50ms
```

### For Specific Port Only (WebRTC media ports)

```bash
# Create root HTB qdisc
sudo tc qdisc add dev $INTERFACE root handle 1: htb default 10

# Create class for WebRTC ports (UDP 49152-65535)
sudo tc class add dev $INTERFACE parent 1: classid 1:10 htb rate 1gbit

# Add netem to WebRTC class
sudo tc qdisc add dev $INTERFACE parent 1:10 handle 10: netem \
    loss 5% delay 50ms 30ms

# Filter UDP traffic to WebRTC ports
sudo tc filter add dev $INTERFACE protocol ip parent 1:0 prio 1 u32 \
    match ip dport 49152 0xffff0000 \
    match ip protocol 17 0xff \
    flowid 1:10
```

---

## Running Tests

### 1. Start Garay Server

```bash
# Terminal 1
cargo run --release
# Or with Docker
docker compose up -d
```

### 2. Run Browser Tests Under Impairment

```bash
# Terminal 2 - Apply impairment
sudo tc qdisc add dev eth0 root netem loss 5% delay 50ms 30ms

# Run browser tests
npm run test:browser -- --reporter=list

# Terminal 2 - Clear impairment
sudo tc qdisc del dev eth0 root
```

### 3. Run Four-Peer Acceptance Test Under Impairment

```bash
# Apply impairment
sudo tc qdisc add dev eth0 root netem loss 5% delay 50ms 30ms

# Run the acceptance test
cargo test four_peer_all_publishers_acceptance --release -- --nocapture

# Clear impairment
sudo tc qdisc del dev eth0 root
```

### 4. Custom Stress Test Script

```bash
#!/bin/bash
# stress-test.sh - Run stress tests across impairment levels

set -euo pipefail

INTERFACE="${1:-eth0}"
SCENARIOS=(
    "baseline:0:0:0:0"
    "mild:1:25:10:0.5"
    "moderate:5:50:30:2"
    "severe:10:100:50:5"
    "extreme:20:250:100:10"
)

cleanup() {
    sudo tc qdisc del dev $INTERFACE root 2>/dev/null || true
}
trap cleanup EXIT

for scenario in "${SCENARIOS[@]}"; do
    IFS=':' read -r name loss delay jitter reorder <<< "$scenario"
    
    echo "=== Testing $name (loss=${loss}%, delay=${delay}ms, jitter=${jitter}ms, reorder=${reorder}%) ==="
    
    if [[ "$name" != "baseline" ]]; then
        sudo tc qdisc add dev $INTERFACE root handle 1: netem \
            loss ${loss}% \
            delay ${delay}ms ${jitter}ms distribution normal \
            reorder ${reorder}% 50%
    fi
    
    # Run acceptance test
    echo "Running four-peer acceptance test..."
    cargo test four_peer_all_publishers_acceptance --release -- --nocapture 2>&1 | tee "stress-${name}.log"
    
    # Clear for next iteration
    sudo tc qdisc del dev $INTERFACE root 2>/dev/null || true
    sleep 2
done

echo "=== All scenarios complete ==="
ls -la stress-*.log
```

---

## Metrics to Capture

### From Garay Metrics (`/metrics`)

```bash
# Capture metrics before, during, after each scenario
curl -s http://localhost:3000/metrics > metrics-${name}-before.txt
# ... run test ...
curl -s http://localhost:3000/metrics > metrics-${name}-after.txt
```

Key metrics to compare:

| Metric | Expected Behavior Under Impairment |
|--------|-----------------------------------|
| `garay_rtp_packets_forwarded_total` | Decreases with loss; should recover |
| `garay_relay_errors_total` | Increases with loss/jitter |
| `garay_peer_connection_failed_total` | May increase with severe impairment |
| `garay_webrtc_negotiation_duration_seconds` | Increases with latency |
| `garay_signaling_messages_received_total` | Retransmissions may increase count |

### From Browser (WebRTC Internals)

In test, capture from `chrome://webrtc-internals` or `about:webrtc`:

- `packetsLost` / `packetsReceived` → **Packet loss %**
- `jitterBufferMs` → **Jitter buffer delay**
- `roundTripTime` → **Measured RTT**
- `nackCount`, `pliCount`, `firCount` → **Recovery requests**
- `bytesReceived` / `bytesSent` → **Bitrate**
- `framesDecoded`, `framesDropped` → **Video quality**

### From OS (tc statistics)

```bash
# Show netem statistics
tc -s qdisc show dev eth0

# Example output:
# qdisc netem 1: root refcnt 2 limit 1000 loss 5% delay 50ms 30ms
#  Sent 123456 bytes 1234 pkt (dropped 61, overlimits 0 requeues 0)
#  backlog 0b 0p requeues 0
```

---

## Expected Results & Thresholds

| Scenario | Max Acceptable Packet Loss | Max Negotiation Time | Min Bitrate (per stream) |
|----------|---------------------------|---------------------|-------------------------|
| Baseline | 0% | < 3s | > 1.5 Mbps |
| Mild (1%) | < 1.5% | < 5s | > 1.2 Mbps |
| Moderate (5%) | < 6% | < 10s | > 800 kbps |
| Severe (10%) | < 12% | < 20s | > 400 kbps |
| Extreme (20%) | < 25% | < 30s | > 200 kbps |

**Failure criteria**:
- Any peer fails to establish connection (ICE failed)
- Packet loss > threshold + 2%
- Negotiation time > threshold × 2
- Bitrate < threshold × 0.5
- Relay errors > 100/min sustained

---

## Automated CI Integration

### GitHub Actions Example

```yaml
# .github/workflows/stress-test.yml
name: Network Stress Tests

on:
  schedule:
    - cron: '0 2 * * 0'  # Weekly Sunday 2AM
  workflow_dispatch:
    inputs:
      scenario:
        type: choice
        options: [mild, moderate, severe]
        default: moderate

jobs:
  stress-test:
    runs-on: ubuntu-latest
    timeout-minutes: 60
    steps:
      - uses: actions/checkout@v4
      
      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable
      
      - name: Install tc
        run: sudo apt-get update && sudo apt-get install -y iproute2
      
      - name: Build release
        run: cargo build --release
      
      - name: Start server
        run: |
          cargo run --release &
          sleep 5
          curl -f http://localhost:3000/health
      
      - name: Apply network impairment
        run: |
          sudo tc qdisc add dev eth0 root netem \
            loss ${{ matrix.loss }}% \
            delay ${{ matrix.delay }}ms ${{ matrix.jitter }}ms \
            reorder ${{ matrix.reorder }}%
        env:
          loss: ${{ fromJson('{"mild":1,"moderate":5,"severe":10}')[github.event.inputs.scenario] }}
          delay: ${{ fromJson('{"mild":25,"moderate":50,"severe":100}')[github.event.inputs.scenario] }}
          jitter: ${{ fromJson('{"mild":10,"moderate":30,"severe":50}')[github.event.inputs.scenario] }}
          reorder: ${{ fromJson('{"mild":0.5,"moderate":2,"severe":5}')[github.event.inputs.scenario] }}
      
      - name: Run acceptance test
        run: cargo test four_peer_all_publishers_acceptance --release -- --nocapture
      
      - name: Collect metrics
        run: curl -s http://localhost:3000/metrics > metrics.txt
      
      - name: Upload artifacts
        uses: actions/upload-artifact@v4
        with:
          name: stress-test-${{ github.event.inputs.scenario }}
          path: |
            metrics.txt
            stress-*.log
```

---

## Cleanup

```bash
# Always clean up after testing
sudo tc qdisc del dev eth0 root 2>/dev/null || true

# Verify clean
tc qdisc show dev eth0
# Should show only: qdisc pfifo_fast 0: root ...
```

---

## Troubleshooting

| Issue | Solution |
|-------|----------|
| `RTNETLINK answers: Operation not permitted` | Run with `sudo` |
| `RTNETLINK answers: Device or resource busy` | `tc qdisc del dev eth0 root` first |
| No effect on traffic | Check correct interface (`ip link show`); ensure not in container namespace |
| Container networking | Apply on host `docker0` or use `nsenter` |
| IPv6 not affected | Add `protocol ipv6` filters or use `tc qdisc add dev eth0 root netem ...` (affects both) |

---

## Safety Notes

- **Test on staging first** - impairment affects ALL traffic on interface
- **Use dedicated test interface** - create dummy interface: `ip link add dummy0 type dummy`
- **Limit scope** - use port filters to only impair WebRTC traffic
- **Monitor production** - never run on production interfaces