# ZeroClaw Remote Diagnostics — Troubleshooting Guide

This guide explains how to start, verify, and debug the full local stack. It covers
expected vs. unexpected failures, the inference pipeline, command patterns, and common
symptoms with their fixes.

---

## Table of Contents

1. [Architecture Overview](#1-architecture-overview)
2. [Starting the Stack](#2-starting-the-stack)
3. [Verifying Services Are Healthy](#3-verifying-services-are-healthy)
4. [Understanding the Inference Pipeline](#4-understanding-the-inference-pipeline)
5. [Commands That Succeed vs. Commands That Fail](#5-commands-that-succeed-vs-commands-that-fail)
6. [Exact Command Patterns (Rule-Based Engine)](#6-exact-command-patterns-rule-based-engine)
7. [Bedrock Inference — Setup and Testing](#7-bedrock-inference--setup-and-testing)
8. [In-Memory State Reset After Restart](#8-in-memory-state-reset-after-restart)
9. [Frontend Troubleshooting](#9-frontend-troubleshooting)
10. [MQTT Troubleshooting](#10-mqtt-troubleshooting)
11. [Fleet Agent / Ollama Troubleshooting](#11-fleet-agent--ollama-troubleshooting)
12. [Symptom Index](#12-symptom-index)

---

## 1. Architecture Overview

Understanding the data flow prevents most confusion:

```
Browser
  │  HTTP + WebSocket
  ▼
Frontend (SvelteKit :5174)
  │  /api proxy
  ▼
Cloud API (Axum :3000)
  │  Inference engine parses NL command → ParsedIntent
  │  Publishes envelope via MQTT
  ▼
MQTT Broker (Mosquitto :1883)
  │
  ▼
Fleet Agent (zc-fleet-agent)
  │  Routes ParsedIntent by ActionKind:
  │    Tool  → ToolRegistry (10 tools: 5 CAN + 5 log)
  │    Shell → Safe shell executor (allowlist enforced)
  │    Reply → Returns message directly
  │
  ▼  Publishes CommandResponse via MQTT
Cloud API (receives response, stores it, broadcasts via WebSocket)
  │
  ▼
Browser (CommandForm receives WS push, displays result)
```

**Three inference layers** (all parse the same natural-language command):

| Layer | Where | Engine | Latency | Cost |
|-------|-------|--------|---------|------|
| Cloud rule-based | Cloud API | Pattern matching | <1 ms | $0 |
| Cloud Bedrock | Cloud API | AWS Nova Lite (LLM) | 200–1500 ms | ~$0.001/query |
| Edge Ollama | Fleet Agent | phi3:mini (local LLM) | 50–500 ms | $0 |

The cloud API uses **one engine at a time** controlled by `INFERENCE_ENGINE`:
- `local` (default) — rule-based engine only
- `bedrock` — Bedrock only

The fleet agent **always** runs Ollama on-device as a fallback if the cloud sends a
command without a `parsed_intent`.

---

## 2. Starting the Stack

Four processes must run. Open separate terminals for each.

### Terminal 1 — Mosquitto

```bash
# Check if already running:
pgrep -a mosquitto

# Start if not running:
mosquitto -p 1883 -v
```

### Terminal 2 — Cloud API (local inference)

```bash
cd /path/to/zeroclaw-remote-diagnostics

INFERENCE_ENGINE=local \
MQTT_ENABLED=true \
MQTT_FLEET_ID=local-fleet \
MQTT_BROKER_HOST=localhost \
MQTT_BROKER_PORT=1883 \
MQTT_USE_TLS=false \
RUST_LOG=info \
target/debug/zc-cloud-api
```

Wait for: `"listening","addr":"0.0.0.0:3000"`

### Terminal 2 (alternative) — Cloud API with Bedrock

```bash
INFERENCE_ENGINE=bedrock \
BEDROCK_MODEL_ID=us.amazon.nova-lite-v1:0 \
AWS_ACCESS_KEY_ID=<your-key> \
AWS_SECRET_ACCESS_KEY=<your-secret> \
AWS_DEFAULT_REGION=us-east-2 \
MQTT_ENABLED=true \
MQTT_FLEET_ID=local-fleet \
MQTT_BROKER_HOST=localhost \
MQTT_BROKER_PORT=1883 \
MQTT_USE_TLS=false \
RUST_LOG=info \
target/debug/zc-cloud-api
```

Wait for: `"inference engine active","inference_tier":"bedrock"`

### Terminal 3 — Fleet Agent

```bash
RUST_LOG=info target/debug/zc-fleet-agent dev/agent.toml
```

Wait for: `"zc-fleet-agent ready"`

### Terminal 4 — Frontend

```bash
cd frontend
API_URL=http://localhost:3000 pnpm dev -- --port 5174
```

Wait for: `VITE ready in ... ms` then open http://localhost:5174

> **Note**: If port 5174 is taken, Vite increments — check the terminal output for the
> actual port (e.g., 5175).

---

## 3. Verifying Services Are Healthy

Run these checks before sending commands.

### Check all ports are listening

```bash
ss -tlnp | grep -E "1883|3000|5174"
```

Expected:
```
LISTEN  0.0.0.0:1883   ← Mosquitto
LISTEN  0.0.0.0:3000   ← Cloud API
LISTEN  127.0.0.1:5174 ← Frontend (or 5175/5176 if port was taken)
```

### Check Cloud API health

```bash
curl -s http://localhost:3000/health
# → {"status":"ok","version":"0.1.0"}
```

### Check which inference engine is active

The startup log tells you:

```bash
# Grep from the cloud API log (or watch the terminal):
grep "inference" /tmp/cloud-api.log
```

| Log message | Meaning |
|-------------|---------|
| `"inference engine: rule-based (local patterns)"` | `INFERENCE_ENGINE=local` |
| `"inference engine: bedrock (cloud LLM)"` | `INFERENCE_ENGINE=bedrock` |
| `"inference_tier":"local"` in a command response | Rule-based matched it |
| `"inference_tier":"bedrock"` in a command response | Bedrock classified it |

### Check device is registered

```bash
curl -s http://localhost:3000/api/v1/devices | python3 -m json.tool
```

If `dev-001` is missing (happens after a restart because state is in-memory), provision it:

```bash
curl -s http://localhost:3000/api/v1/devices \
  -H 'Content-Type: application/json' \
  -d '{
    "device_id": "dev-001",
    "fleet_id": "local-fleet",
    "hardware_type": "x86-dev",
    "vin": "TEST00000000001"
  }' | python3 -m json.tool
```

The fleet agent's heartbeat (every 10 s) will flip the status from `provisioning` → `online` automatically.

### Check fleet agent is connected

In the fleet agent terminal, you should see periodic lines like:

```json
{"message":"published heartbeat","uptime_secs":30}
{"message":"shadow state reported","version":3}
```

If you see `"mqtt connection error"` — Mosquitto is not running or the port is wrong.

---

## 4. Understanding the Inference Pipeline

### How a command travels

1. You type `"check disk space"` in the browser and click Send.
2. The frontend POSTs to `POST /api/v1/commands`.
3. The Cloud API calls the active inference engine to parse the text into a `ParsedIntent`:
   - `action`: `tool` | `shell` | `reply`
   - `tool_name`: the tool or shell command to run
   - `tool_args`: JSON arguments
   - `confidence`: 0.0–1.0
4. The Cloud API publishes the command envelope (with `parsed_intent` embedded) to MQTT:
   `fleet/local-fleet/device/dev-001/commands`
5. The Fleet Agent receives it, reads `parsed_intent`, and routes:
   - `Tool` → looks up the tool in the registry, calls `execute(args, backend)`
   - `Shell` → validates and runs the command through the safe shell executor
   - `Reply` → extracts the message from `tool_args["message"]`, returns it directly
6. The Fleet Agent publishes a `CommandResponse` back to MQTT.
7. The Cloud API MQTT bridge receives it, stores the result, broadcasts a `WsEvent::CommandResponse` via WebSocket.
8. The frontend CommandForm receives the WS event and renders the result.

### When Ollama runs (edge inference)

If the cloud sends a command **without** a `parsed_intent` (e.g., the inference engine
returned `None`), the fleet agent calls its local Ollama model (`phi3:mini` by default)
to parse the command. This is the edge-side inference fallback.

Ollama is **not** involved when the cloud engine already produced a `parsed_intent`.

---

## 5. Commands That Succeed vs. Commands That Fail

### Expected failures — not bugs

| Command example | Result | Reason |
|----------------|--------|--------|
| "read the DTCs" | `failed: Response timeout after 2000ms` | MockCanInterface — no real CAN hardware |
| "read the VIN" | `failed: Response timeout after 2000ms` | MockCanInterface |
| "read RPM" | `failed: Response timeout after 2000ms` | MockCanInterface |
| "read coolant temperature" | `failed: Response timeout after 2000ms` | MockCanInterface |
| "monitor CAN bus" | `failed: Response timeout after 2000ms` | MockCanInterface |
| "read freeze frame" | `failed: Response timeout after 2000ms` | MockCanInterface |

**All CAN/OBD-II tools always fail in local dev** because the fleet agent uses
`MockCanInterface` (a test double that simulates CAN timeouts). This is correct
behaviour — real hardware is not connected. This will be replaced with `SocketCanInterface`
in a future phase.

### Commands that succeed

#### Log tools (real syslog on the host)

| Command example | Tool called | Notes |
|----------------|-------------|-------|
| `"search logs for error"` | `search_logs` | Scans `/var/log/syslog` |
| `"search logs for connection refused"` | `search_logs` | Query extracted from "for ..." |
| `"analyze errors in the logs"` | `analyze_errors` | 9 error categories |
| `"show log statistics"` | `log_stats` | Count by severity |
| `"tail the logs"` | `tail_logs` | Last 50 lines |
| `"show recent logs 100"` | `tail_logs` | Last 100 lines |
| `"show journal for nginx.service"` | `query_journal` | Calls `journalctl` |
| `"service logs for sshd"` | `query_journal` | Unit name extracted |

#### Shell commands (run on the device, allowlisted)

| Command example | Shell command run | Notes |
|----------------|------------------|-------|
| `"what is the system uptime?"` | `uptime` | |
| `"check disk space"` | `df -h` | |
| `"show memory usage"` | `free -h` | |
| `"what processes are running?"` | `ps aux` | |
| `"what is the kernel version?"` | `uname -a` | |
| `"show cpu info"` | `lscpu` | |
| `"what is the hostname?"` | `hostname` | |
| `"what is the ip address?"` | `ip -brief addr` | |
| `"what is the cpu temperature?"` | `cat /sys/class/thermal/thermal_zone0/temp` | May error if thermal_zone0 absent on x86 |
| `"what is the gpu temperature?"` | `vcgencmd measure_temp` | Raspberry Pi only |

#### Conversational (reply — no execution)

| Command example | Notes |
|----------------|-------|
| `"hello, what can you do?"` | Returns a capabilities summary |
| `"how are you?"` | Returns status message |
| `"what is your purpose?"` | Conversational response |

---

## 6. Exact Command Patterns (Rule-Based Engine)

When `INFERENCE_ENGINE=local`, the Cloud API uses exact substring matching (case-insensitive).
If your phrasing does not match, the rule engine returns `None` — and with Bedrock disabled,
the command reaches the fleet agent without a `parsed_intent`, so Ollama parses it on-device.

### Pattern reference

#### CAN / OBD-II tools

| To call | Your phrase must contain one of |
|---------|--------------------------------|
| `read_dtcs` | `"read dtc"`, `"get dtc"`, `"trouble code"`, `"engine code"`, `"check code"`, `"fault code"` |
| `read_vin` | `"read vin"`, `"get vin"`, `"vehicle identification"`, `"show vin"`, `"what is the vin"` |
| `read_freeze` | `"freeze frame"`, `"freeze data"`, `"snapshot data"`, `"read freeze"` |
| `can_monitor` | `"monitor can"`, `"sniff can"`, `"capture can"`, `"can bus traffic"`, `"can traffic"`, `"bus monitor"` |
| `read_pid` (RPM) | `"rpm"` or `"engine speed"` or `"engine rpm"` + a verb (`read`/`get`/`show`/`what`/`check`) |
| `read_pid` (speed) | `"speed"` or `"vehicle speed"` + verb |
| `read_pid` (coolant) | `"coolant"` or `"coolant temp"` or `"engine temp"` + verb |
| `read_pid` (throttle) | `"throttle"` or `"throttle position"` + verb |
| `read_pid` (fuel) | `"fuel level"` or `"fuel"` + verb |

#### Log tools

| To call | Your phrase must contain one of |
|---------|--------------------------------|
| `search_logs` | `"search log"`, `"grep log"`, `"find in log"`, `"search for"` |
| `analyze_errors` | `"analyze error"`, `"error analysis"`, `"what error"`, `"find error"`, `"show error"` |
| `log_stats` | `"log stat"`, `"log summar"`, `"log overview"`, `"show stat"` |
| `tail_logs` | `"tail log"`, `"recent log"`, `"latest log"`, `"show log"`, `"last log"` |
| `query_journal` | `"journal for"`, `"journalctl"`, `"service log"`, `"systemd log"`, `"show journal"` |

#### Shell commands

| To call | Your phrase must contain one of |
|---------|--------------------------------|
| `ip -brief addr` | `"ip address"`, `"ip addr"`, `"network interface"`, `"network info"` |
| `cat /sys/.../temp` | `"cpu temp"`, `"cpu temperature"`, `"processor temp"` |
| `vcgencmd measure_temp` | `"gpu temp"`, `"gpu temperature"` |
| `df -h` | `"disk space"`, `"disk usage"`, `"storage"`, `"free space"` |
| `free -h` | `"memory"`, `"ram"`, `"free mem"` |
| `uptime` | `"uptime"` (exact substring anywhere in command) |
| `uname -a` | `"kernel version"`, `"kernel"`, `"uname"` |
| `lscpu` | `"cpu info"`, `"processor info"`, `"lscpu"` |
| `ps aux` | `"process"`, `"running process"`, `"what's running"` |
| `hostname` | `"hostname"` (exact substring anywhere in command) |

> **Tip**: Ambiguous phrases like "is the powertrain healthy?" will not match the
> rule-based engine and will fall through to Bedrock (if enabled) or Ollama (on-device).

---

## 7. Bedrock Inference — Setup and Testing

### Prerequisites

```bash
# Verify AWS credentials
aws sts get-caller-identity

# Verify Bedrock access (a ValidationException is fine — AccessDeniedException is not)
aws bedrock-runtime invoke-model \
  --model-id us.amazon.nova-lite-v1:0 \
  --body '{}' 2>&1 | head -3
```

### Environment variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `INFERENCE_ENGINE` | Yes | `local` | Set to `bedrock` to enable Bedrock |
| `AWS_ACCESS_KEY_ID` | Yes | — | IAM key |
| `AWS_SECRET_ACCESS_KEY` | Yes | — | IAM secret |
| `AWS_DEFAULT_REGION` | Yes | from profile | Region (must support Nova Lite, e.g. `us-east-2`) |
| `BEDROCK_MODEL_ID` | No | `us.amazon.nova-lite-v1:0` | Model to use |
| `BEDROCK_TIMEOUT_SECS` | No | `15` | Per-request timeout (cold starts can take 8–10 s) |

### Confirming Bedrock is active

Check startup log:
```
"inference engine: bedrock (cloud LLM)"
"bedrock model configured","model_id":"us.amazon.nova-lite-v1:0"
"aws region resolved","region":"us-east-2"
"inference engine active","inference_tier":"bedrock"
```

Check a command response — the `inference_tier` field will be `"bedrock"`:
```bash
curl -s http://localhost:3000/api/v1/commands/<id> | python3 -m json.tool | grep inference_tier
```

### What Bedrock adds over the rule-based engine

The rule-based engine covers ~80% of commands with exact pattern matching. Bedrock handles
the rest — ambiguous, colloquial, or domain-specific phrasing that has no exact pattern:

| Phrase | Rule-based | Bedrock |
|--------|-----------|---------|
| `"read DTCs"` | ✅ matched | ✅ matched (redundant) |
| `"is the powertrain healthy?"` | ❌ no match | ✅ → `read_dtcs` |
| `"how's the engine doing?"` | ❌ no match | ✅ → `read_dtcs` or `reply` |
| `"check disk space"` | ✅ matched | ✅ matched |
| `"what's eating all my disk?"` | ❌ no match | ✅ → `df -h` (shell) |
| `"show errors from last boot"` | ❌ no match | ✅ → `analyze_errors` or `query_journal` |

> **Important**: Even with Bedrock, CAN tools (`read_dtcs`, `read_pid`, etc.) will still
> return `"Response timeout after 2000ms"` because the fleet agent uses MockCanInterface.
> Bedrock correctly classified the command — the failure is at hardware execution.

### Bedrock failure modes

| Symptom | Cause | Fix |
|---------|-------|-----|
| `"inference_tier":"local"` despite `INFERENCE_ENGINE=bedrock` | Wrong env var value (use `bedrock` not `true`) | Check startup log |
| `bedrock converse error: AccessDeniedException` | IAM policy missing `bedrock:InvokeModel` | Add permission in AWS Console |
| `bedrock converse error: ResourceNotFoundException` | Wrong region or model ID | Set `AWS_DEFAULT_REGION` to `us-east-1` or `us-east-2` |
| `bedrock inference timed out` | Cold start or slow response | Increase `BEDROCK_TIMEOUT_SECS=30` |
| Bedrock returns a shell command with pipes | Expected — sanitized automatically | The executor strips everything from the first `|`, `;`, etc. |

---

## 8. In-Memory State Reset After Restart

The Cloud API runs without a database (`DATABASE_URL` not set). All state — devices,
commands, telemetry, shadows — lives in memory and is **lost on restart**.

After every restart you must re-provision `dev-001`:

```bash
curl -s http://localhost:3000/api/v1/devices \
  -H 'Content-Type: application/json' \
  -d '{"device_id":"dev-001","fleet_id":"local-fleet","hardware_type":"x86-dev","vin":"TEST00000000001"}' \
  | python3 -m json.tool
```

The sample devices (`rpi-001`, `rpi-002`, `sbc-010`) are pre-loaded from `AppState::with_sample_data()`
on startup — they always exist. `dev-001` is the live fleet agent and must be provisioned manually.

The fleet agent's heartbeat runs every 10 seconds. After provisioning, wait ~10 s and the
device status will flip to `online` in the API and frontend.

---

## 9. Frontend Troubleshooting

### "Live" indicator is grey / "Connecting"

The WebSocket to the Cloud API is not connected.

1. Confirm the Cloud API is running: `curl -s http://localhost:3000/health`
2. Confirm the frontend proxy is pointed at the right port:
   - `API_URL=http://localhost:3000` must match the Cloud API port
   - Check the terminal that started the frontend for Vite proxy errors

### "device not found" in the command form

`dev-001` was not provisioned. See [Section 8](#8-in-memory-state-reset-after-restart).

### Frontend started on wrong port (5175 or 5176)

Port 5174 was already in use. Find and kill the occupant:

```bash
# See what's on 5174:
ss -tlnp | grep 5174

# Kill it (replace PID):
kill <pid>
```

Then restart with `pnpm dev -- --port 5174`.

### Command shows "dispatched" but no response arrives

The fleet agent is not running or is not connected to MQTT. Check:

```bash
# Fleet agent running?
pgrep -a zc-fleet-agent

# Mosquitto running?
pgrep -a mosquitto
```

The command will time out after 60 s in the UI (the "Waiting for response" indicator
shows elapsed time). After timeout you can still check the command via the API:

```bash
curl -s http://localhost:3000/api/v1/commands/<id> | python3 -m json.tool
```

---

## 10. MQTT Troubleshooting

### Mosquitto not starting

```bash
# Check if port 1883 is already bound:
ss -tlnp | grep 1883

# If another mosquitto instance is running:
pkill mosquitto
mosquitto -p 1883 -v
```

### Cloud API cannot connect to MQTT

Look for in the Cloud API log:
```
"mqtt connection error"
```

Check:
- `MQTT_BROKER_HOST=localhost` is set
- `MQTT_BROKER_PORT=1883` matches the running broker
- `MQTT_ENABLED=true` is set (without this the MQTT bridge doesn't start)

### Fleet agent cannot connect to MQTT

Check `dev/agent.toml`:
```toml
[mqtt]
broker_host = "localhost"
broker_port = 1883
use_tls = false
```

If `use_tls = true`, the agent will try mTLS with certificates. For local dev, keep `use_tls = false`.

### MQTT payload size errors

AWS IoT Core enforces a 128 KB MQTT payload limit. The Cloud API's `cap_response_size`
function trims oversized tool responses before publishing. If you see log entries like:

```
"response truncated to fit mqtt limit"
```

This is expected for very large log analysis results. The truncated summary is still
returned in `response_text`.

---

## 11. Fleet Agent / Ollama Troubleshooting

### Ollama not running

If the fleet agent starts without Ollama available, commands without a pre-parsed intent
will not be classified. The agent logs:

```
"ollama request failed","error":"connection refused"
```

Start Ollama:
```bash
ollama serve &
ollama pull phi3:mini   # first time only
```

Verify:
```bash
ollama list   # should show phi3:mini
curl -s http://localhost:11434/api/tags | python3 -m json.tool
```

### Ollama returns unknown tool

phi3:mini sometimes generates tool names not in the allowlist (`KNOWN_TOOLS`). The
agent logs:
```
"ollama returned unknown tool","tool_name":"self_destruct"
```

The intent is silently dropped and the command fails. This is the validation layer
working correctly.

### Ollama is slow (>500 ms)

phi3:mini typically runs in 50–200 ms on modern hardware. If it's slower:
- Check available RAM: `free -h` (model needs ~2 GB)
- Check CPU load: `top`
- Consider a smaller quantized model: `ollama pull phi3:mini:q4_0`

### Fleet agent connected but commands not received

The fleet agent subscribes to:
- `fleet/local-fleet/device/dev-001/commands`
- `fleet/local-fleet/broadcast/commands`

Ensure the fleet agent `fleet_id` and `device_id` in `dev/agent.toml` match what the
Cloud API uses (`local-fleet` / `dev-001`). A mismatch means the agent is on a different
MQTT topic and never sees the command.

### Shell command blocked unexpectedly

The shell executor enforces a strict allowlist. Allowed commands:

```
cat  ls  df  free  uname  uptime  ps  ip  ifconfig  hostname  sensors
lscpu  lsblk  head  tail  wc  du  ss  date  dmesg  journalctl  systemctl
vcgencmd  top  whoami
```

`systemctl` is further restricted to read-only subcommands: `status`, `is-active`,
`is-enabled`, `list-units`, `show`.

Shell metacharacters are always blocked regardless of command:
`;`, `|`, `` ` ``, `$(`, `>`, `<`, `&&`, `||`

If a Bedrock or Ollama-generated command includes pipes, the executor strips everything
from the first metacharacter onward before running it.

---

## 12. Symptom Index

| Symptom | Section | Fix |
|---------|---------|-----|
| `"device not found"` in UI | §8 | Re-provision dev-001 after restart |
| `Response timeout after 2000ms` for CAN commands | §5 | Expected — MockCanInterface, no real hardware |
| `inference_tier:"local"` even with `INFERENCE_ENGINE=bedrock` | §7 | Wrong env var value |
| `bedrock converse error: AccessDeniedException` | §7 | Add `bedrock:InvokeModel` IAM permission |
| Frontend shows "Connecting" / grey dot | §9 | Cloud API not running or wrong API_URL port |
| Frontend on port 5175 instead of 5174 | §9 | Kill whatever owns 5174, restart frontend |
| Commands dispatched but no response | §9, §10 | Fleet agent or Mosquitto not running |
| Fleet agent logs `"connection refused"` | §10 | Mosquitto not running |
| Fleet agent logs `"ollama request failed"` | §11 | `ollama serve` not running |
| Shell command rejected with `"blocked command: rm"` | §11 | Intentional security block |
| Shell command rejected with `"shell injection detected"` | §11 | Command contains `|`, `;`, etc. |
| All devices missing after Cloud API restart | §8 | In-memory state lost — re-provision dev-001 |
| Sample devices (rpi-001, rpi-002, sbc-010) missing | §8 | Cloud API not started — check logs |
| `"aws region resolved"` not in startup log | §7 | `AWS_DEFAULT_REGION` not set |
| Ollama command returns wrong tool name | §11 | phi3 hallucination — commands validated, safely dropped |
| `"response truncated"` in logs | §10 | Response was >128 KB, trimmed to fit MQTT limit |
