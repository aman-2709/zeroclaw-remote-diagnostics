# ZeroClaw Remote Diagnostics

Intelligent command-and-control platform for IoT device fleets (primarily connected vehicles). Combines edge-side AI inference with cloud fallback for remote diagnostics, log analysis, and natural-language device interaction.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│  Operator Dashboard (SvelteKit)                                 │
│  Fleet view · Device detail · NL command interface · Real-time  │
└──────────────┬──────────────────────────────────┬───────────────┘
               │ REST API                         │ WebSocket
┌──────────────▼──────────────────────────────────▼───────────────┐
│  Cloud API (Rust / Axum)                                        │
│  Command dispatch · Response ingestion · NL inference engine    │
│  PostgreSQL storage · WebSocket broadcast                       │
└──────────────┬──────────────────────────────────────────────────┘
               │ MQTT (AWS IoT Core, mTLS)
┌──────────────▼──────────────────────────────────────────────────┐
│  Edge Agent (Rust / ZeroClaw)                                   │
│  CAN bus tools (8) · Log tools (5) · MQTT channel · Heartbeat  │
│  Inference chain: Ollama → Bedrock (opt) → Fallback · Agent mode│
└─────────────────────────────────────────────────────────────────┘
```

### Three Layers

| Layer | Stack | Purpose |
|-------|-------|---------|
| **Edge** | Rust (ZeroClaw runtime), Ollama, Bedrock (optional) | On-device AI inference chain, CAN/OBD-II diagnostics, log analysis |
| **Cloud** | Rust (Axum), PostgreSQL, AWS IoT Core, AWS Bedrock | Command routing, NL inference fallback, device registry, telemetry |
| **Frontend** | SvelteKit 5, Tailwind CSS 4 | Fleet dashboard, device management, real-time command interface |

### Inference Strategy

**Cloud API** uses one inference engine at a time, configured via `INFERENCE_ENGINE` env var:

| Engine | Env Value | Handles | Latency | Cost |
|--------|-----------|---------|---------|------|
| Rule-based (local) | `local` (default) | Pattern matching for 10 tools + 10 shell commands, ~80% coverage | <1 ms | $0 |
| Bedrock (cloud) | `bedrock` | Complex/ambiguous queries via AWS Converse API | 200–1500 ms | ~$0.001/query |

**Edge agent** uses a trait-based `EdgeInferenceEngine` chain for commands that arrive without a pre-parsed intent. Engines are tried in order; first match wins:

| Engine | Activation | Handles | Latency | Cost |
|--------|-----------|---------|---------|------|
| Ollama | `[ollama] enabled = true` | Local LLM (phi3:mini) | 50–500 ms | $0 |
| Bedrock | `--features bedrock` + `[bedrock] enabled = true` | Cloud LLM fallback (Nova Lite) | 200–1500 ms | ~$0.001/query |
| Fallback | always active | Keyword matching (greetings, help, thanks) | <1 ms | $0 |

### Inference Configuration Reference

Three independent configuration points control where inference happens:

| What | Where | How to configure |
|------|-------|-----------------|
| **Cloud inference** | Cloud API server | `INFERENCE_ENGINE` env var: `local` (default), `bedrock`, `tiered` |
| **Edge Ollama** | Fleet agent (agent.toml) | `[ollama] enabled = true\|false` |
| **Edge Bedrock** | Fleet agent (agent.toml + compile flag) | `cargo build --features bedrock` + `[bedrock] enabled = true\|false` |

Cloud engines run first. Edge engines are a safety net for commands that arrive without a pre-parsed intent.

#### Common Configurations

**1. Smart edge, dumb cloud (recommended for autonomous agents)**

Cloud does rules only. Edge agent handles everything else via Bedrock.

```bash
# Cloud API
INFERENCE_ENGINE=local cargo run -p zc-cloud-api
```
```toml
# agent.toml
[ollama]
enabled = false

[bedrock]
enabled = true
region = "us-east-2"
model_id = "us.amazon.nova-lite-v1:0"
timeout_secs = 15
```
```bash
cargo run -p zc-fleet-agent --features bedrock -- agent.toml
```
Engine chain: `rules (cloud) → bedrock (edge) → fallback (edge)`

**2. Cloud handles everything (cheapest, simplest)**

Cloud Bedrock parses all commands. Edge just executes pre-parsed intents.

```bash
# Cloud API
INFERENCE_ENGINE=tiered cargo run -p zc-cloud-api
```
```toml
# agent.toml
[ollama]
enabled = false
# No [bedrock] section needed
```
```bash
cargo run -p zc-fleet-agent -- agent.toml   # no --features bedrock needed
```
Engine chain: `rules (cloud) → bedrock (cloud) → fallback (edge)`

**3. Fully offline (no cloud LLM cost)**

Ollama runs on-device. No Bedrock anywhere.

```bash
# Cloud API
INFERENCE_ENGINE=local cargo run -p zc-cloud-api
```
```toml
# agent.toml
[ollama]
enabled = true
host = "http://localhost:11434"
model = "phi3:mini"
timeout_secs = 10
```
```bash
cargo run -p zc-fleet-agent -- agent.toml
```
Engine chain: `rules (cloud) → ollama (edge) → fallback (edge)`

**4. Maximum resilience (Ollama + Bedrock fallback)**

Ollama handles most queries for free. Bedrock catches what Ollama misses.

```toml
# agent.toml
[ollama]
enabled = true
timeout_secs = 10

[bedrock]
enabled = true
region = "us-east-2"
```
```bash
cargo run -p zc-fleet-agent --features bedrock -- agent.toml
```
Engine chain: `rules (cloud) → ollama (edge) → bedrock (edge) → fallback (edge)`

**5. No-GPU device (e.g., S32G)**

Device can't run Ollama. Edge Bedrock handles unparsed commands directly.

```toml
# agent.toml
[ollama]
enabled = false

[bedrock]
enabled = true
region = "us-east-2"
```
Engine chain: `rules (cloud) → bedrock (edge) → fallback (edge)`

## Project Structure

```
crates/
  zc-protocol/        Shared types: commands, telemetry, device, DTC, shadows, topics
  zc-canbus-tools/    CAN bus / OBD-II diagnostic tools (5 tools, trait-based)
  zc-log-tools/       Multi-format log parsing + 5 analysis tools
  zc-mqtt-channel/    MQTT channel abstraction for AWS IoT Core (mTLS)
  zc-fleet-agent/     Edge agent binary (wires all crates + MQTT event loop)
  zc-cloud-api/       Cloud API server (Axum REST, PostgreSQL/SQLx, WebSocket)
infra/
  modules/
    networking/        VPC, subnets (public/private), NAT, routing
    iot-core/          Thing types, thing groups, IoT policies, topic rules
    compute/           Lambda (Rust/AL2023/ARM64), API Gateway HTTP API
    data/              RDS PostgreSQL 16, Secrets Manager
    monitoring/        CloudWatch alarms, dashboard
frontend/              SvelteKit 5 + Tailwind CSS 4 (SPA, adapter-static)
```

## Edge Tools

### CAN Bus Tools (`zc-canbus-tools`)

| Tool | Description |
|------|-------------|
| `read_pid` | Read OBD-II parameter IDs (RPM, speed, temp, fuel, throttle) |
| `read_dtcs` | Read diagnostic trouble codes |
| `read_vin` | Read vehicle identification number (multi-frame ISO-TP) |
| `read_freeze` | Read freeze frame data for stored DTCs |
| `can_monitor` | Monitor raw CAN bus traffic with optional ID filtering |

### Log Tools (`zc-log-tools`)

| Tool | Description |
|------|-------------|
| `search_logs` | Regex search across log files with severity filtering |
| `analyze_errors` | Classify errors into 9 categories (connection, permission, resource, etc.) |
| `log_stats` | Aggregate statistics: severity distribution, top sources, time range |
| `tail_logs` | Tail recent log entries with optional severity filter |
| `query_journal` | Query systemd journal by unit name (runs `journalctl --output=export`) |

Supports 4 log formats with auto-detection: syslog (RFC 3164/5424), journald, JSON lines, plaintext.

## Cloud API Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/health` | Health check |
| `GET` | `/api/v1/devices` | List all devices |
| `GET` | `/api/v1/devices/{id}` | Get device details |
| `POST` | `/api/v1/commands` | Dispatch a NL command to a device |
| `GET` | `/api/v1/commands` | List recent commands |
| `GET` | `/api/v1/commands/{id}` | Get command status and response |
| `POST` | `/api/v1/commands/{id}/respond` | Ingest command response from device |
| `POST` | `/api/v1/heartbeat` | Ingest device heartbeat |
| `GET/POST` | `/api/v1/devices/{id}/telemetry` | Get / ingest telemetry |
| `GET` | `/api/v1/devices/{id}/shadows` | List device shadows |
| `GET` | `/api/v1/devices/{id}/shadows/{name}` | Get shadow (reported + desired + delta) |
| `PUT` | `/api/v1/devices/{id}/shadows/{name}/desired` | Set desired state (publishes delta) |
| `GET` | `/api/v1/ws` | WebSocket for real-time events |

### WebSocket Events

- `command_dispatched` — new command sent to device
- `command_response` — device response received (includes `response_data`, `error`)
- `device_heartbeat` — device heartbeat received
- `device_status_changed` — device status transition
- `telemetry_ingested` — telemetry batch received
- `shadow_updated` — device shadow state changed

## Getting Started

### Prerequisites

- Rust (edition 2024)
- Node.js + pnpm
- PostgreSQL 16 (optional — tests run with in-memory fallback)

### Build & Test

```bash
# Build all crates
cargo build --workspace

# Run all tests (621 tests, no external dependencies required)
cargo test --workspace

# Build fleet agent with Bedrock edge inference (feature-gated)
cargo build -p zc-fleet-agent --features bedrock

# Lint
cargo clippy --workspace -- -D warnings

# Format
cargo fmt --all
cargo fmt --all -- --check   # check only
```

### Local Dev (Full Loop)

Requires Mosquitto MQTT broker running on `localhost:1883`.

```bash
# Terminal 1: MQTT broker
mosquitto -p 1883 -v

# Terminal 2: Cloud API with MQTT bridge (rule-based inference)
INFERENCE_ENGINE=local \
PORT=3002 \
MQTT_ENABLED=true \
MQTT_FLEET_ID=local-fleet \
MQTT_BROKER_HOST=localhost \
MQTT_BROKER_PORT=1883 \
MQTT_USE_TLS=false \
RUST_LOG=info \
cargo run -p zc-cloud-api

# Terminal 3: Fleet agent
RUST_LOG=info cargo run -p zc-fleet-agent -- dev/agent.toml

# Terminal 4: Frontend dev server
cd frontend && pnpm install && pnpm dev -- --port 5174
```

To use Bedrock cloud inference instead, set `INFERENCE_ENGINE=bedrock` plus AWS credentials in Terminal 2 (see [Bedrock Cloud Inference](#bedrock-cloud-inference) below).

To enable Bedrock on the **edge agent**, build with `--features bedrock` and uncomment the `[bedrock]` section in `dev/agent.toml` (see [Edge Bedrock Inference](#edge-bedrock-inference) below).

### Run the Cloud API

```bash
# Without database (in-memory mode with sample data)
cargo run -p zc-cloud-api

# With PostgreSQL
DATABASE_URL=postgres://user:pass@localhost/zeroclaw cargo run -p zc-cloud-api
```

### Bedrock Cloud Inference

To use AWS Bedrock instead of the local rule-based engine, set `INFERENCE_ENGINE=bedrock`. Requires AWS credentials with `bedrock:InvokeModel` permission and model access enabled in the Bedrock console.

```bash
# Cloud API with Bedrock + MQTT (full stack, no database)
INFERENCE_ENGINE=bedrock \
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

**Environment variables:**

| Variable | Default | Description |
|----------|---------|-------------|
| `INFERENCE_ENGINE` | `local` | Inference engine: `local` (rule-based) or `bedrock` (cloud LLM) |
| `BEDROCK_MODEL_ID` | `us.amazon.nova-lite-v1:0` | Bedrock model ID (only when `INFERENCE_ENGINE=bedrock`) |
| `BEDROCK_TIMEOUT_SECS` | `15` | Per-request timeout (cold starts can take 8-10s) |
| `AWS_ACCESS_KEY_ID` | from profile | AWS access key |
| `AWS_SECRET_ACCESS_KEY` | from profile | AWS secret key |
| `AWS_DEFAULT_REGION` | from profile | AWS region (must support chosen model) |

Startup logs confirm the active engine:
```
"inference engine: bedrock (cloud LLM)"    # INFERENCE_ENGINE=bedrock
"inference engine: local (rule-based)"     # INFERENCE_ENGINE=local (default)
```

### Edge Bedrock Inference

The fleet agent supports an optional Bedrock engine in its inference chain, feature-gated behind `--features bedrock`. When enabled, the chain is: Ollama (free) → Bedrock (cloud) → Fallback (keyword). This lets devices without Ollama/GPU still handle complex queries via cloud LLM.

```bash
# Build with Bedrock support
cargo build -p zc-fleet-agent --features bedrock

# Run (AWS credentials from environment)
RUST_LOG=info cargo run -p zc-fleet-agent --features bedrock -- dev/agent.toml
```

Enable in `dev/agent.toml`:

```toml
[bedrock]
enabled = true
region = "us-east-1"            # must support the chosen model
model_id = "us.amazon.nova-lite-v1:0"
timeout_secs = 15
```

The `[bedrock]` section is always deserializable (config parsing doesn't require the feature flag). The engine is only instantiated when compiled with `--features bedrock` **and** `enabled = true`.

Requires the same AWS credentials and `bedrock:InvokeModel` permission as the cloud Bedrock engine.

### Run the Frontend

```bash
cd frontend/
pnpm install
pnpm dev          # Dev server (proxies /api to localhost:3000)
pnpm check        # Type check
pnpm build        # Production build
```

### Infrastructure (Terraform)

```bash
cd infra/
cp terraform.tfvars.example terraform.tfvars  # Edit with your values
terraform init
terraform validate
terraform plan -var-file=terraform.tfvars
terraform apply -var-file=terraform.tfvars
```

## Command Lifecycle

```
Operator  ──NL command──▶  Cloud API  ──inference──▶  ParsedIntent
                              │                            │
                              ├──store (PostgreSQL)────────┘
                              ├──broadcast (WS: command_dispatched)
                              └──publish (MQTT)──▶  Edge Agent
                                                       │
                                                  tool dispatch
                                                       │
Edge Agent  ──CommandResponse──▶  Cloud API
                                      │
                                      ├──update DB (status, response, latency)
                                      ├──broadcast (WS: command_response)
                                      └──return 200
```

## Key Design Decisions

1. **Extension crates, not fork** — implements ZeroClaw traits rather than forking upstream
2. **Dual-mode AppState** — optional `PgPool` + in-memory fallback (all tests pass without a database)
3. **Trait abstractions** — `CanInterface`, `LogSource`, `Channel` with mock implementations for testing
4. **Edge-first build order** — highest-risk components (CAN, MQTT, agent) built before cloud/frontend
5. **IoT Core over FleetWise** — full MQTT control, custom message format, lower cost at PoC scale

## Security Model

- Per-device X.509 certificates with mTLS (AWS IoT Core)
- Read-only CAN bus mode (no ECU writes until security model validated)
- Command allowlisting and workspace scoping (ZeroClaw)
- TLS 1.3 everywhere, credentials in AWS Secrets Manager
- Full command audit trail

## Success Criteria (PoC)

| Metric | Target |
|--------|--------|
| Local inference p95 latency | <200 ms |
| Bedrock fallback p95 latency | <2000 ms |
| Local query coverage | >=80% |
| Per-device monthly cost | <$5 (at 50 devices) |
| Command success rate | >98% |
| Unauthorized command prevention | 100% blocked |
| Log analysis accuracy | >95% error/warning detection |

## License

MIT OR Apache-2.0
