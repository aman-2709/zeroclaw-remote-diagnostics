# Local Full-Loop Manual Test

Test the complete command lifecycle: curl → cloud API → MQTT → fleet agent → action execution → response back.

The fleet agent supports three action types (Phase 8 — Agent Mode):
- **Tool** — routes to one of 10 diagnostic tools (5 CAN + 5 log)
- **Shell** — runs a safe system command on the device (allowlisted, injection-blocked)
- **Reply** — conversational response, no action taken

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

### Terminal 2 — Cloud API (rule-based inference only)

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

### Terminal 2 — Cloud API (with Bedrock fallback)

Add AWS credentials and `BEDROCK_ENABLED=true` to enable the tiered inference engine (rule-based → Bedrock):

```bash
BEDROCK_ENABLED=true \
BEDROCK_MODEL_ID=us.amazon.nova-lite-v1:0 \
AWS_ACCESS_KEY_ID=AKIA... \
AWS_SECRET_ACCESS_KEY=... \
AWS_DEFAULT_REGION=us-east-2 \
PORT=3002 \
MQTT_ENABLED=true \
MQTT_FLEET_ID=local-fleet \
MQTT_BROKER_HOST=localhost \
MQTT_BROKER_PORT=1883 \
MQTT_USE_TLS=false \
RUST_LOG=info \
cargo run -p zc-cloud-api
```

Wait for: `"inference engine active","inference_tier":"tiered"`

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

## 4. Test Tool Actions (diagnostic tools)

Ollama routes these to one of the 10 registered tools.

### 4a. Log tool (should succeed)

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

### 4b. CAN tool (expected: mock timeout)

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

Expected: `"status": "failed"`, `"error": "Response timeout after 2000ms"`. This is correct — `MockCanInterface` simulates a timeout since there's no real CAN hardware.

### 4c. More tool commands

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

### What to watch in Terminal 3 (fleet agent):
- `"received command"` — agent picked it up from MQTT
- `"ollama parsed command locally"` — phi3:mini identified the action
- `"command completed"` — action executed and response sent back

## 5. Test Shell Actions (system commands)

Ollama routes these to the safe shell executor. Commands are validated against an allowlist (21 commands including `uptime`, `df`, `free`, `ps`, `cat`, `ls`, etc.). Dangerous commands (`rm`, `kill`, `sudo`, etc.) are blocked.

### 5a. System uptime (should succeed)

```bash
curl -s http://localhost:3002/api/v1/commands \
  -H 'Content-Type: application/json' \
  -d '{
    "device_id": "dev-001",
    "fleet_id": "local-fleet",
    "command": "what is the system uptime?",
    "initiated_by": "aman"
  }' | python3 -m json.tool
```

Expected: `"status": "completed"`, `"response_text"` contains uptime output.

### 5b. Disk space (should succeed)

```bash
curl -s http://localhost:3002/api/v1/commands \
  -H 'Content-Type: application/json' \
  -d '{
    "device_id": "dev-001",
    "fleet_id": "local-fleet",
    "command": "check disk space usage",
    "initiated_by": "aman"
  }' | python3 -m json.tool
```

Expected: `"status": "completed"`, `"response_text"` contains `df` output.

### 5c. Dangerous command (should be blocked)

```bash
curl -s http://localhost:3002/api/v1/commands \
  -H 'Content-Type: application/json' \
  -d '{
    "device_id": "dev-001",
    "fleet_id": "local-fleet",
    "command": "delete all files in /tmp",
    "initiated_by": "aman"
  }' | python3 -m json.tool
```

Expected: `"status": "failed"`, `"error": "shell: blocked command: rm"`. The shell executor blocks destructive commands.

## 6. Test Reply Actions (conversational)

Ollama returns a conversational response without executing any tool or command.

```bash
curl -s http://localhost:3002/api/v1/commands \
  -H 'Content-Type: application/json' \
  -d '{
    "device_id": "dev-001",
    "fleet_id": "local-fleet",
    "command": "hello, what can you do?",
    "initiated_by": "aman"
  }' | python3 -m json.tool
```

Expected: `"status": "completed"`, `"response_text"` contains a friendly greeting from the agent (e.g. "Hello! I'm the fleet agent for this device.").

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
- Connection indicator shows "Live" (green dot, WebSocket connected)
- Click `dev-001` → device detail page with Command Interface
- Send a tool command (e.g. "show me log statistics") — should show Action: Tool, Tool name, Args, Confidence, then response
- Send a shell command (e.g. "what is the system uptime?") — should show "Command sent to device" then uptime output
- Send a reply command (e.g. "hello, what can you do?") — should show conversational response
- Commands page shows full audit trail with status badges (completed/failed)
- Shadow data visible at `/api/v1/devices/dev-001/shadows/diagnostics`

## Quick Reference

| Service | URL | Port |
|---------|-----|------|
| Cloud API | http://localhost:3002 | 3002 |
| Frontend | http://localhost:5174 | 5174 |
| Mosquitto | localhost | 1883 |
| Ollama | http://localhost:11434 | 11434 |

| Action | Command Example | Expected |
|--------|----------------|----------|
| **Tool** | "search logs for errors" | success (real syslog) |
| **Tool** | "analyze errors in logs" | success |
| **Tool** | "show log statistics" | success |
| **Tool** | "tail the system logs" | success |
| **Tool** | "read DTCs" | fail (mock CAN timeout) |
| **Tool** | "read engine RPM" | fail (mock CAN timeout) |
| **Tool** | "read VIN" | fail (mock CAN timeout) |
| **Tool** | "read freeze frame" | fail (mock CAN timeout) |
| **Tool** | "monitor CAN bus" | fail (mock CAN timeout) |
| **Shell** | "what is the system uptime?" | success (uptime output) |
| **Shell** | "check disk space usage" | success (df output) |
| **Shell** | "show memory usage" | success (free output) |
| **Shell** | "list running processes" | success (ps output) |
| **Shell** | "delete all files in /tmp" | fail (blocked: rm) |
| **Reply** | "hello, what can you do?" | success (conversational) |
| **Reply** | "how are you?" | success (conversational) |

## 10. Bedrock Cloud Inference

Test the Bedrock fallback path (TieredEngine: rule-based → Bedrock). Requires AWS credentials with `bedrock:InvokeModel` permission.

### Prerequisites

```bash
# AWS credentials configured (SSO, env vars, or ~/.aws/credentials)
aws sts get-caller-identity          # should return your account
aws bedrock-runtime invoke-model \
  --model-id us.amazon.nova-lite-v1:0 \
  --body '{}' --query 'stop' 2>&1 | head -1
# ^ just checking access — a validation error is fine, "AccessDenied" is not
```

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `BEDROCK_ENABLED` | `false` | Set `true` to build TieredEngine |
| `BEDROCK_MODEL_ID` | `us.amazon.nova-lite-v1:0` | Bedrock model ID |
| `BEDROCK_TIMEOUT_SECS` | `15` | Per-request timeout (cold starts ~8-10s) |
| `AWS_DEFAULT_REGION` | from profile | Must support the chosen model |

### Start Cloud API with Bedrock

```bash
BEDROCK_ENABLED=true \
AWS_DEFAULT_REGION=us-east-1 \
PORT=3002 \
MQTT_ENABLED=true \
MQTT_FLEET_ID=local-fleet \
MQTT_BROKER_HOST=localhost \
MQTT_BROKER_PORT=1883 \
MQTT_USE_TLS=false \
RUST_LOG=info \
cargo run -p zc-cloud-api
```

Verify startup logs show:
```
"aws region resolved","region":"us-east-1"
"inference engine active","inference_tier":"tiered"
```

Without `BEDROCK_ENABLED=true`, logs show `"inference_tier":"rule_based"`.

### Test: Rule-Based Hit (no Bedrock call)

```bash
curl -s http://localhost:3002/api/v1/commands \
  -H 'Content-Type: application/json' \
  -d '{
    "device_id": "dev-001",
    "fleet_id": "local-fleet",
    "command": "read the DTCs",
    "initiated_by": "aman"
  }' | python3 -m json.tool
```

Expected: `"inference_tier": "local"` — the rule-based engine matched, Bedrock was not called.

### Test: Bedrock Fallback (ambiguous command)

```bash
curl -s http://localhost:3002/api/v1/commands \
  -H 'Content-Type: application/json' \
  -d '{
    "device_id": "dev-001",
    "fleet_id": "local-fleet",
    "command": "is the powertrain healthy?",
    "initiated_by": "aman"
  }' | python3 -m json.tool
```

Expected: `"inference_tier": "bedrock"` — rule-based engine missed, Bedrock classified it (likely as a tool or reply action).

### Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| `inference_tier: "rule_based"` despite BEDROCK_ENABLED | Missing env var | Ensure `BEDROCK_ENABLED=true` (not `1`) |
| `bedrock inference timed out` | Cold start or slow network | Increase `BEDROCK_TIMEOUT_SECS=30` |
| `bedrock converse error: AccessDenied` | IAM permissions | Add `bedrock:InvokeModel` to your role/user |
| `bedrock converse error: ResourceNotFound` | Wrong region/model | Check `AWS_DEFAULT_REGION` supports `BEDROCK_MODEL_ID` |

## Cleanup

Ctrl-C in Terminal 2 and Terminal 3 to stop cloud API and fleet agent.
Mosquitto: `pkill mosquitto` or Ctrl-C if running in foreground.
