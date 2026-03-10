# Raspberry Pi / ARM64 Deployment Guide

Deploy the ZeroClaw fleet agent to an ARM64 device (Raspberry Pi, NXP S32G, etc.) on the same subnet as your dev machine.

## Architecture

```
Dev machine (192.168.x.Y)               ARM64 device (192.168.x.Z)
┌──────────────────────────┐             ┌──────────────────────┐
│ Frontend     (:5173)     │             │ zc-fleet-agent       │
│ Cloud API    (:3002)     │◄── MQTT ───►│ Ollama (optional)    │
│ Mosquitto    (:1883)     │             │                      │
└──────────────────────────┘             └──────────────────────┘
```

The dev machine runs the Cloud API, MQTT broker, and frontend. The ARM64 device runs only the fleet agent, which connects back to your machine over MQTT (plaintext, no TLS — local dev only).

---

## Prerequisites

### Dev machine

1. **Mosquitto** listening on all interfaces (not just localhost):

```bash
echo -e "listener 1883 0.0.0.0\nallow_anonymous true" | sudo tee /etc/mosquitto/conf.d/zeroclaw.conf
sudo systemctl restart mosquitto

# Verify
ss -tlnp | grep 1883
# Should show: 0.0.0.0:1883  (not 127.0.0.1:1883)
```

2. **Cloud API + frontend** running:

```bash
# Terminal 1
./dev/run-local.sh

# Terminal 2
cd frontend && pnpm dev
```

3. Know your machine's IP:

```bash
ip -4 route get 1 | awk '{print $7; exit}'
# e.g., 192.168.62.111
```

### ARM64 device

- Ubuntu/Debian (aarch64)
- SSH access
- Network connectivity to your dev machine on port 1883

---

## Option A: Build natively on the device

Best for: devices with >=1.5 GB RAM, or when cross-compilation has glibc issues (e.g., Ubuntu 20.04 with old glibc).

### 1. Install Rust on the device

```bash
ssh <user>@<device-ip>

# Install curl if missing
sudo apt-get update && sudo apt-get install -y curl gcc

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source $HOME/.cargo/env
rustc --version   # should be >= 1.85
```

### 2. Copy source code

From your dev machine:

```bash
ROOT="$(pwd)"   # zeroclaw-remote-diagnostics repo root
DEVICE="<user>@<device-ip>"
REMOTE_DIR="/opt/zeroclaw"

# Create directory on device
ssh $DEVICE "sudo mkdir -p $REMOTE_DIR && sudo chown \$(whoami) $REMOTE_DIR"

# Create source tarball (only the crates needed for the fleet agent)
tar czf /tmp/zc-source.tar.gz \
  Cargo.lock \
  crates/zc-protocol/ \
  crates/zc-canbus-tools/ \
  crates/zc-log-tools/ \
  crates/zc-mqtt-channel/ \
  crates/zc-fleet-agent/

scp /tmp/zc-source.tar.gz $DEVICE:$REMOTE_DIR/
ssh $DEVICE "cd $REMOTE_DIR && tar xzf zc-source.tar.gz"
```

### 3. Create workspace Cargo.toml on the device

```bash
ssh $DEVICE "cat > $REMOTE_DIR/Cargo.toml" <<'EOF'
[workspace]
resolver = "2"
members = [
    "crates/zc-protocol",
    "crates/zc-canbus-tools",
    "crates/zc-log-tools",
    "crates/zc-mqtt-channel",
    "crates/zc-fleet-agent",
]

[workspace.package]
version = "0.1.0"
edition = "2024"
license = "MIT OR Apache-2.0"

[workspace.dependencies]
tokio = { version = "1.42", features = ["full"] }
async-trait = "0.1"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
anyhow = "1.0"
thiserror = "2.0"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
chrono = { version = "0.4", features = ["serde"] }
uuid = { version = "1.0", features = ["v7", "serde"] }
rumqttc = "0.24"
socketcan = "3.5"
regex = "1.11"
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
zc-protocol = { path = "crates/zc-protocol" }
zc-canbus-tools = { path = "crates/zc-canbus-tools" }
zc-mqtt-channel = { path = "crates/zc-mqtt-channel" }
zc-log-tools = { path = "crates/zc-log-tools" }
zc-fleet-agent = { path = "crates/zc-fleet-agent" }

[profile.release]
lto = true
codegen-units = 1
strip = true
panic = "abort"

[profile.release-edge]
inherits = "release"
opt-level = "s"
EOF
```

### 4. Build

```bash
ssh $DEVICE "source \$HOME/.cargo/env && cd $REMOTE_DIR && cargo build --release -p zc-fleet-agent"
```

This takes **10–20 minutes** on a Cortex-A53 (2 cores). On a Cortex-A72 (Pi 4/5) it's closer to 5–10 minutes. Subsequent rebuilds are much faster (incremental).

The binary will be at `/opt/zeroclaw/target/release/zc-fleet-agent`.

### 5. Copy binary to install location

```bash
ssh $DEVICE "cp $REMOTE_DIR/target/release/zc-fleet-agent $REMOTE_DIR/zc-fleet-agent"
```

---

## Option B: Cross-compile from dev machine

Best for: devices with limited RAM (<1 GB) or when you want fast iteration. Requires matching glibc version or static linking.

### Using `cross` (Docker-based, easiest)

```bash
# Install cross
cargo install cross

# Build (uses a Docker container with matching glibc)
cross build --release --target aarch64-unknown-linux-gnu -p zc-fleet-agent

# Binary at: target/aarch64-unknown-linux-gnu/release/zc-fleet-agent
scp target/aarch64-unknown-linux-gnu/release/zc-fleet-agent $DEVICE:$REMOTE_DIR/
```

Requires Docker with user access (`docker ps` must work without sudo).

### Using musl (fully static, no glibc dependency)

```bash
rustup target add aarch64-unknown-linux-musl

# Need a musl cross-compiler — install via your distro or use a prebuilt toolchain
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=aarch64-linux-musl-gcc \
  cargo build --release --target aarch64-unknown-linux-musl -p zc-fleet-agent
```

### Using gnu (matches dev machine glibc — only works if device glibc is same or newer)

```bash
sudo apt install gcc-aarch64-linux-gnu
rustup target add aarch64-unknown-linux-gnu

CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
  cargo build --release --target aarch64-unknown-linux-gnu -p zc-fleet-agent
```

> **Warning**: This links against your dev machine's glibc. If the device runs an older OS (e.g., Ubuntu 20.04 with glibc 2.31), you'll get `GLIBC_2.xx not found` errors at runtime. Use Option A or musl in that case.

---

## Agent Configuration

Create `/opt/zeroclaw/agent.toml` on the device:

```toml
fleet_id = "fleet-alpha"
device_id = "s32g-001"          # unique per device
can_interface = "can0"           # set to your CAN interface, or omit
heartbeat_interval_secs = 10
shadow_sync_interval_secs = 30
log_paths = ["/var/log/syslog"]

[mqtt]
broker_host = "192.168.62.111"   # <-- your dev machine's IP
broker_port = 1883
client_id = "s32g-001"           # must match device_id
use_tls = false

[ollama]
host = "http://localhost:11434"
model = "phi3:mini"
timeout_secs = 30
enabled = false                  # set to true if Ollama is installed on device
```

Replace:
- `device_id` / `client_id` — unique name for this device
- `broker_host` — your dev machine's LAN IP
- `can_interface` — the CAN interface name (`can0`, `vcan0`, or omit)
- `ollama.enabled` — `true` if you install Ollama on the device

---

## Systemd Service

Install a systemd service so the agent starts on boot and auto-restarts:

```bash
ssh $DEVICE "sudo tee /etc/systemd/system/zeroclaw-agent.service > /dev/null" <<'EOF'
[Unit]
Description=ZeroClaw Fleet Agent
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=/opt/zeroclaw/zc-fleet-agent /opt/zeroclaw/agent.toml
Restart=on-failure
RestartSec=5
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
EOF

ssh $DEVICE "sudo systemctl daemon-reload && sudo systemctl enable zeroclaw-agent && sudo systemctl start zeroclaw-agent"
```

### Verify

```bash
# Check service status
ssh $DEVICE "systemctl status zeroclaw-agent"

# Watch logs
ssh $DEVICE "sudo journalctl -u zeroclaw-agent -f"

# Restart after config changes
ssh $DEVICE "sudo systemctl restart zeroclaw-agent"

# Stop
ssh $DEVICE "sudo systemctl stop zeroclaw-agent"
```

---

## Verification

Once the agent is running and connected, you should see:

### 1. MQTT heartbeats (on dev machine)

```bash
mosquitto_sub -h localhost -p 1883 -t 'fleet/#' -v
```

You should see heartbeat pings every 10 seconds:
```
fleet/fleet-alpha/s32g-001/heartbeat/ping {"device_id":"s32g-001","fleet_id":"fleet-alpha","status":"online",...}
```

### 2. Device in the dashboard

Open `http://localhost:5173` — the device should appear in the device list with a green "online" status.

### 3. Send a command

From the dashboard, select the device and try:

```
show disk space
what is the uptime?
show kernel messages
whoami
```

The command is sent from your browser → Cloud API → MQTT → device agent → executes on the Pi → response back through MQTT → displayed in the dashboard.

### 4. CAN bus commands (if CAN interfaces are available)

If the device has real CAN interfaces (`ip link show type can`):

```
read DTCs
read VIN
read RPM
monitor CAN bus traffic for 10 seconds
show CAN interface state
```

---

## Installing Ollama (optional)

Ollama provides local LLM inference on the device for commands that arrive without a pre-parsed intent. With `INFERENCE_ENGINE=tiered` on the cloud API, most commands are pre-parsed by rules, so Ollama is only needed as a fallback for edge-only operation.

```bash
ssh $DEVICE

# Install Ollama
curl -fsSL https://ollama.com/install.sh | sh

# Pull the model (~1.5 GB download, needs ~2 GB RAM to run)
ollama pull phi3:mini

# Enable in agent config
sed -i 's/enabled = false/enabled = true/' /opt/zeroclaw/agent.toml

# Restart agent
sudo systemctl restart zeroclaw-agent
```

> **Note**: phi3:mini requires ~2 GB RAM. On devices with <=2 GB RAM, consider disabling Ollama and relying on cloud-side inference (tiered mode).

---

## Troubleshooting

### Agent crashes with `GLIBC_2.xx not found`

The binary was cross-compiled against a newer glibc than the device has. Fix: build natively on the device (Option A) or use musl static linking.

### Agent can't connect to MQTT

1. Check Mosquitto is on `0.0.0.0:1883`: `ss -tlnp | grep 1883` on dev machine
2. Check network: `ping <dev-machine-ip>` from device
3. Check firewall: `sudo ufw status` on dev machine — port 1883 must be open
4. Check agent config: `broker_host` must be the dev machine's LAN IP, not `localhost`

### Device shows "offline" in dashboard

1. Verify heartbeats: `mosquitto_sub -t 'fleet/#' -v` on dev machine
2. Check Cloud API has MQTT enabled: look for `mqtt subscriptions established` in API logs
3. Restart the Cloud API after Mosquitto: the API must connect after the broker is ready

### CAN tools return errors

1. Check CAN interfaces exist: `ip link show type can` on device
2. Check CAN is UP: `ip -details link show can0`
3. The agent uses SocketCAN — the interface must be configured and up before the agent starts

### Shell commands fail with "command not allowed"

The agent enforces a strict allowlist of read-only commands. See `crates/zc-fleet-agent/src/shell.rs` for the full list. Commands not on the allowlist are blocked for security.
