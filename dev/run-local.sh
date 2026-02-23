#!/usr/bin/env bash
# Start all services for local full-loop testing.
# Usage: ./dev/run-local.sh
#
# Starts: cloud API (:3002), fleet agent, frontend (:5174)
# Requires: mosquitto running on :1883, Ollama running on :11434
# Ctrl-C stops everything.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
API_PORT=3002
FRONTEND_PORT=5174
PIDS=()

cleanup() {
    echo ""
    echo "Shutting down..."
    for pid in "${PIDS[@]}"; do
        kill "$pid" 2>/dev/null || true
    done
    wait 2>/dev/null
    echo "All services stopped."
}
trap cleanup EXIT INT TERM

echo "=== Starting local dev stack ==="

# 1. Check mosquitto
if pgrep -x mosquitto >/dev/null 2>&1; then
    echo "[1/4] Mosquitto already running on :1883"
else
    echo "[1/4] Starting mosquitto on :1883..."
    mosquitto -p 1883 -d
    sleep 0.5
fi

# 2. Check Ollama
if curl -sf http://localhost:11434/api/tags >/dev/null 2>&1; then
    echo "[2/4] Ollama running on :11434"
else
    echo "[2/4] WARNING: Ollama not reachable on :11434 — local inference will fail"
fi

# 3. Cloud API
echo "[3/4] Starting cloud API on :${API_PORT}..."
PORT=$API_PORT \
MQTT_ENABLED=true \
MQTT_FLEET_ID=local-fleet \
MQTT_BROKER_HOST=localhost \
MQTT_BROKER_PORT=1883 \
MQTT_USE_TLS=false \
RUST_LOG=info \
cargo run -p zc-cloud-api &
PIDS+=($!)
sleep 3

# 4. Fleet agent
echo "[4/4] Starting fleet agent..."
RUST_LOG=info \
cargo run -p zc-fleet-agent -- "$ROOT/dev/agent.toml" &
PIDS+=($!)
sleep 2

echo ""
echo "=== Services running ==="
echo "  Cloud API:  http://localhost:${API_PORT}"
echo "  Mosquitto:  localhost:1883"
echo "  Ollama:     http://localhost:11434"
echo ""
echo "To start frontend (separate terminal):"
echo "  cd frontend && API_URL=http://localhost:${API_PORT} pnpm dev -- --port ${FRONTEND_PORT}"
echo ""
echo "Press Ctrl-C to stop all services."
wait
