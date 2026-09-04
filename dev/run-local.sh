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
MOSQUITTO_CONF=""
MOSQUITTO_CONTAINER=""

cleanup() {
    echo ""
    echo "Shutting down..."
    for pid in "${PIDS[@]}"; do
        kill "$pid" 2>/dev/null || true
    done
    wait 2>/dev/null
    if [[ -n "$MOSQUITTO_CONTAINER" ]]; then
        docker rm -f "$MOSQUITTO_CONTAINER" >/dev/null 2>&1 || true
    fi
    if [[ -n "$MOSQUITTO_CONF" ]]; then
        rm -f "$MOSQUITTO_CONF"
    fi
    echo "All services stopped."
}
trap cleanup EXIT INT TERM

echo "=== Starting local dev stack ==="

# 1. Check mosquitto — bind to 0.0.0.0 so remote devices (e.g. Pi) can connect
if pgrep -x mosquitto >/dev/null 2>&1; then
    echo "[1/4] Mosquitto already running on :1883"
elif command -v mosquitto >/dev/null 2>&1; then
    echo "[1/4] Starting mosquitto on 0.0.0.0:1883..."
    MOSQUITTO_CONF=$(mktemp)
    printf '%s\n' 'listener 1883 0.0.0.0' 'allow_anonymous true' > "$MOSQUITTO_CONF"
    mosquitto -c "$MOSQUITTO_CONF" -d
    sleep 0.5
elif command -v docker >/dev/null 2>&1; then
    echo "[1/4] Starting Mosquitto in Docker on 0.0.0.0:1883..."
    MOSQUITTO_CONTAINER="zc-remote-diagnostics-mosquitto-${PPID}"
    MOSQUITTO_CONF=$(mktemp)
    printf '%s\n' 'listener 1883 0.0.0.0' 'allow_anonymous true' > "$MOSQUITTO_CONF"
    docker rm -f "$MOSQUITTO_CONTAINER" >/dev/null 2>&1 || true
    docker run -d --rm --name "$MOSQUITTO_CONTAINER" -p 1883:1883 \
        -v "$MOSQUITTO_CONF:/mosquitto/config/mosquitto.conf:ro" \
        eclipse-mosquitto:2 >/dev/null
    for _ in {1..10}; do
        if docker exec "$MOSQUITTO_CONTAINER" mosquitto_pub -h 127.0.0.1 -t zc/health -m ready >/dev/null 2>&1; then
            break
        fi
        sleep 1
    done
else
    echo "ERROR: install mosquitto or Docker before starting the local stack" >&2
    exit 1
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
INFERENCE_ENGINE=tiered \
AWS_DEFAULT_REGION=${AWS_DEFAULT_REGION:-us-east-2} \
MQTT_ENABLED=true \
MQTT_FLEET_ID=fleet-alpha \
MQTT_BROKER_HOST=localhost \
MQTT_BROKER_PORT=1883 \
MQTT_USE_TLS=false \
ALLOW_INSECURE=true \
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
echo "  cd frontend && API_URL=http://localhost:${API_PORT} pnpm exec vite --host 127.0.0.1 --port ${FRONTEND_PORT}"
echo ""
echo "Press Ctrl-C to stop all services."
wait
