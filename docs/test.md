# Local Full-Loop Manual Test

Test the complete command lifecycle: curl → cloud API → MQTT → fleet agent → tool execution → response back.

## Prerequisites

```bash
# Verify these are running/installed:
mosquitto -h               # should print version
ollama list                 # should show phi3:mini
cargo --version             # Rust toolchain
```

## 1. Start Services

Open **3 separate terminals**. Run each in the project root.

### Terminal 1 — Mosquitto (if not already running)

```bash
# Check if already running:
pgrep -a mosquitto

# If not, start it:
mosquitto -p 1883 -v
# -v for verbose — you'll see every MQTT publish/subscribe
```

### Terminal 2 — Cloud API

```bash
PORT=3002 \
MQTT_ENABLED=true \
MQTT_FLEET_ID=local-fleet \
MQTT_BROKER_HOST=localhost \
MQTT_BROKER_PORT=1883 \
MQTT_USE_TLS=false \
RUST_LOG=info \
cargo run -p zc-cloud-api
```

Wait for: `"listening","addr":"0.0.0.0:3002"`

### Terminal 3 — Fleet Agent

```bash
RUST_LOG=info cargo run -p zc-fleet-agent -- dev/agent.toml
```

Wait for: `"zc-fleet-agent ready"`

## 2. Health Check

```bash
curl -s http://localhost:3002/health | python3 -m json.tool
```

Expected:
```json
{ "status": "ok", "version": "0.1.0" }
```

## 3. Provision a Device

```bash
curl -s http://localhost:3002/api/v1/devices \
  -H 'Content-Type: application/json' \
  -d '{
    "device_id": "dev-001",
    "fleet_id": "local-fleet",
    "hardware_type": "x86-dev",
    "vin": "TEST00000000001"
  }' | python3 -m json.tool
```

Expected: 201 response with device info, `"status": "provisioning"`.

## 4. Send a Log Command (should succeed)

```bash
curl -s http://localhost:3002/api/v1/commands \
  -H 'Content-Type: application/json' \
  -d '{
    "device_id": "dev-001",
    "fleet_id": "local-fleet",
    "command": "search logs for error messages",
    "initiated_by": "aman"
  }' | python3 -m json.tool
```

Note the `"id"` from the response, then wait ~3 seconds and check:

```bash
curl -s http://localhost:3002/api/v1/commands/<COMMAND_ID> | python3 -m json.tool
```

Expected in `"response"`:
- `"status": "completed"`
- `"inference_tier": "local"` (Ollama parsed it)
- `"response_data"` with `"tool_name": "search_logs"`, `"success": true`

### What to watch in Terminal 3 (fleet agent):
- `"received command"` — agent picked it up from MQTT
- `"ollama parsed command locally"` — phi3:mini identified the tool
- `"command completed"` — tool executed and response sent back

## 5. Send a CAN Command (expected: mock timeout)

```bash
curl -s http://localhost:3002/api/v1/commands \
  -H 'Content-Type: application/json' \
  -d '{
    "device_id": "dev-001",
    "fleet_id": "local-fleet",
    "command": "read the DTCs from this vehicle",
    "initiated_by": "aman"
  }' | python3 -m json.tool
```

Check the response after ~3 seconds (same pattern as above).

Expected in `"response"`:
- `"status": "failed"`
- `"error": "Response timeout after 2000ms"`
- This is correct — `MockCanInterface` simulates a timeout since there's no real CAN hardware.

## 6. Try Other Commands

These should all be routed by Ollama to the right tool:

```bash
# Analyze errors in syslog
curl -s http://localhost:3002/api/v1/commands \
  -H 'Content-Type: application/json' \
  -d '{
    "device_id": "dev-001",
    "fleet_id": "local-fleet",
    "command": "analyze errors in the system logs",
    "initiated_by": "aman"
  }' | python3 -m json.tool

# Log statistics
curl -s http://localhost:3002/api/v1/commands \
  -H 'Content-Type: application/json' \
  -d '{
    "device_id": "dev-001",
    "fleet_id": "local-fleet",
    "command": "show me log statistics",
    "initiated_by": "aman"
  }' | python3 -m json.tool

# Read vehicle VIN (will fail — mock CAN)
curl -s http://localhost:3002/api/v1/commands \
  -H 'Content-Type: application/json' \
  -d '{
    "device_id": "dev-001",
    "fleet_id": "local-fleet",
    "command": "read the VIN number",
    "initiated_by": "aman"
  }' | python3 -m json.tool
```

## 7. Check Heartbeats

The fleet agent sends heartbeats every 10 seconds. After ~15 seconds:

```bash
curl -s http://localhost:3002/api/v1/devices | python3 -m json.tool
```

Look for device status updates or check mosquitto verbose output for heartbeat publishes on `fleet/local-fleet/device/dev-001/heartbeat`.

## 8. List Commands

```bash
curl -s http://localhost:3002/api/v1/commands | python3 -m json.tool
```

Shows all commands with their responses — your full audit trail.

## 9. Frontend (Optional)

Open a **4th terminal**:

```bash
cd frontend
API_URL=http://localhost:3002 pnpm dev -- --port 5174
```

Open http://localhost:5174 in your browser. The frontend proxies all `/api` and WebSocket requests to the cloud API on :3002.

Things to verify:
- Device list shows `dev-001` with recent `last_heartbeat`
- Sending a command from the CommandForm shows "waiting for response" then the result
- Connection indicator shows "Live" (WebSocket connected)
- Shadow data visible at `/api/v1/devices/dev-001/shadows/diagnostics`

## Quick Reference

| Service | URL | Port |
|---------|-----|------|
| Cloud API | http://localhost:3002 | 3002 |
| Frontend | http://localhost:5174 | 5174 |
| Mosquitto | localhost | 1883 |
| Ollama | http://localhost:11434 | 11434 |

| Tool | Command Example | Expected |
|------|----------------|----------|
| search_logs | "search logs for errors" | success (real syslog) |
| analyze_errors | "analyze errors in logs" | success |
| log_stats | "show log statistics" | success |
| tail_logs | "tail the system logs" | success |
| read_dtcs | "read DTCs" | fail (mock CAN timeout) |
| read_pid | "read engine RPM" | fail (mock CAN timeout) |
| read_vin | "read VIN" | fail (mock CAN timeout) |
| read_freeze | "read freeze frame" | fail (mock CAN timeout) |
| can_monitor | "monitor CAN bus" | fail (mock CAN timeout) |

## Cleanup

Ctrl-C in Terminal 2 and Terminal 3 to stop cloud API and fleet agent.
Mosquitto: `pkill mosquitto` or Ctrl-C if running in foreground.
