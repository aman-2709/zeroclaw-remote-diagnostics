#!/usr/bin/env bash
# Deploy ZeroClaw fleet agent to a Raspberry Pi over SSH.
#
# Usage:
#   ./dev/deploy-pi.sh <pi-user>@<pi-ip> [device-id]
#
# Example:
#   ./dev/deploy-pi.sh ubuntu@192.168.62.50
#   ./dev/deploy-pi.sh ubuntu@192.168.62.50 rpi-002
#
# What this does:
#   1. Cross-compiles the fleet agent for ARM64 (aarch64)
#   2. Generates an agent.toml pointing at YOUR machine's Mosquitto
#   3. Copies binary + config to the Pi via scp
#   4. Installs a systemd service on the Pi
#   5. Starts the agent
#
# Prerequisites (on your dev machine):
#   - Rust cross-compilation target: rustup target add aarch64-unknown-linux-gnu
#   - Cross-linker: sudo apt install gcc-aarch64-linux-gnu
#   - SSH access to the Pi (ssh-copy-id recommended)
#
# Prerequisites (on the Pi):
#   - Ubuntu (aarch64) — tested on 22.04/24.04
#   - Ollama (optional): curl -fsSL https://ollama.com/install.sh | sh && ollama pull phi3:mini

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

info()  { echo -e "${GREEN}[INFO]${NC}  $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC}  $*"; }
error() { echo -e "${RED}[ERROR]${NC} $*"; exit 1; }

# --- Args ---
PI_SSH="${1:?Usage: $0 <user>@<pi-ip> [device-id]}"
DEVICE_ID="${2:-rpi-$(echo "$PI_SSH" | grep -oP '\d+$' | head -1)}"
# Fallback if no digits found in IP
[[ -z "$DEVICE_ID" || "$DEVICE_ID" == "rpi-" ]] && DEVICE_ID="rpi-001"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
FLEET_ID="fleet-alpha"
TARGET="aarch64-unknown-linux-gnu"
BINARY_NAME="zc-fleet-agent"
REMOTE_DIR="/opt/zeroclaw"
REMOTE_CONFIG="$REMOTE_DIR/agent.toml"

# --- Detect this machine's IP (the MQTT broker host) ---
BROKER_IP=$(ip -4 route get 1 | awk '{print $7; exit}')
if [[ -z "$BROKER_IP" ]]; then
    error "Could not detect local IP. Set BROKER_IP env var manually."
fi

info "=== ZeroClaw Pi Deployment ==="
info "  Pi:         $PI_SSH"
info "  Device ID:  $DEVICE_ID"
info "  Fleet ID:   $FLEET_ID"
info "  Broker IP:  $BROKER_IP (this machine)"
echo ""

# --- Step 1: Ensure cross-compilation toolchain ---
info "[1/5] Checking cross-compilation toolchain..."
if ! rustup target list --installed | grep -q "$TARGET"; then
    info "Adding target $TARGET..."
    rustup target add "$TARGET"
fi

if ! command -v aarch64-linux-gnu-gcc &>/dev/null; then
    error "Cross-linker not found. Install it:\n  sudo apt install gcc-aarch64-linux-gnu"
fi

# --- Step 2: Cross-compile ---
info "[2/5] Cross-compiling fleet agent for ARM64..."
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
cargo build --release --target "$TARGET" -p zc-fleet-agent 2>&1 | tail -5

BINARY="$ROOT/target/$TARGET/release/$BINARY_NAME"
if [[ ! -f "$BINARY" ]]; then
    error "Binary not found at $BINARY"
fi
BINARY_SIZE=$(du -h "$BINARY" | cut -f1)
info "Binary built: $BINARY ($BINARY_SIZE)"

# --- Step 3: Generate agent.toml ---
info "[3/5] Generating agent config..."
AGENT_TOML=$(mktemp)
cat > "$AGENT_TOML" <<EOF
fleet_id = "$FLEET_ID"
device_id = "$DEVICE_ID"
heartbeat_interval_secs = 10
shadow_sync_interval_secs = 30
log_paths = ["/var/log/syslog"]

[mqtt]
broker_host = "$BROKER_IP"
broker_port = 1883
client_id = "$DEVICE_ID"
use_tls = false

[ollama]
host = "http://localhost:11434"
model = "phi3:mini"
timeout_secs = 30
enabled = true
EOF

info "Config generated (broker → $BROKER_IP:1883)"
cat "$AGENT_TOML"
echo ""

# --- Step 4: Copy to Pi ---
info "[4/5] Deploying to Pi..."
ssh "$PI_SSH" "sudo mkdir -p $REMOTE_DIR && sudo chown \$(whoami) $REMOTE_DIR"
scp "$BINARY" "$PI_SSH:$REMOTE_DIR/$BINARY_NAME"
scp "$AGENT_TOML" "$PI_SSH:$REMOTE_DIR/agent.toml"
rm -f "$AGENT_TOML"

# Make binary executable
ssh "$PI_SSH" "chmod +x $REMOTE_DIR/$BINARY_NAME"

info "Files deployed to $PI_SSH:$REMOTE_DIR/"

# --- Step 5: Install and start systemd service ---
info "[5/5] Installing systemd service..."
ssh "$PI_SSH" "sudo tee /etc/systemd/system/zeroclaw-agent.service > /dev/null" <<EOF
[Unit]
Description=ZeroClaw Fleet Agent
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=$REMOTE_DIR/$BINARY_NAME $REMOTE_CONFIG
Restart=on-failure
RestartSec=5
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
EOF

ssh "$PI_SSH" "sudo systemctl daemon-reload && sudo systemctl enable zeroclaw-agent && sudo systemctl restart zeroclaw-agent"

# Wait a moment and check status
sleep 2
STATUS=$(ssh "$PI_SSH" "systemctl is-active zeroclaw-agent 2>/dev/null || echo 'failed'")

echo ""
info "=========================================="
if [[ "$STATUS" == "active" ]]; then
    info "Agent running on Pi!"
else
    warn "Agent may not have started. Check with:"
    echo "  ssh $PI_SSH 'journalctl -u zeroclaw-agent -f'"
fi
info "=========================================="
echo ""
info "Device ID:    $DEVICE_ID"
info "Fleet ID:     $FLEET_ID"
info "MQTT Broker:  $BROKER_IP:1883"
echo ""
info "Useful commands:"
echo "  ssh $PI_SSH 'journalctl -u zeroclaw-agent -f'     # watch agent logs"
echo "  ssh $PI_SSH 'systemctl status zeroclaw-agent'      # check status"
echo "  ssh $PI_SSH 'systemctl restart zeroclaw-agent'     # restart"
echo "  ssh $PI_SSH 'systemctl stop zeroclaw-agent'        # stop"
echo ""
info "On your dev machine, make sure:"
echo "  1. Mosquitto listens on 0.0.0.0:1883 (not just localhost)"
echo "  2. Cloud API is running: ./dev/run-local.sh"
echo "  3. Frontend is running:  cd frontend && pnpm dev"
echo ""
info "Then open http://localhost:5173 — you should see '$DEVICE_ID' appear!"
