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
│  CAN bus tools (5) · Log tools (4) · MQTT channel · Heartbeat  │
│  Local LLM inference (Ollama) · Tool dispatch                   │
└─────────────────────────────────────────────────────────────────┘
```

### Three Layers

| Layer | Stack | Purpose |
|-------|-------|---------|
| **Edge** | Rust (ZeroClaw runtime), Ollama | On-device AI inference, CAN/OBD-II diagnostics, log analysis |
| **Cloud** | Rust (Axum), PostgreSQL, AWS IoT Core, AWS Bedrock | Command routing, NL inference fallback, device registry, telemetry |
| **Frontend** | SvelteKit 5, Tailwind CSS 4 | Fleet dashboard, device management, real-time command interface |

### Hybrid Inference Strategy

| Tier | Engine | Handles | Latency | Cost |
|------|--------|---------|---------|------|
| Tier 1 | Rule-based (local) | Structured commands, known patterns (~80% of queries) | <1 ms | $0 |
| Tier 2 | AWS Bedrock (cloud) | Complex reasoning, ambiguous commands | 200-1500 ms | ~$0.001/query |

The `TieredEngine` tries local inference first, falls back to Bedrock only when local parsing returns no result.

## Project Structure

```
crates/
  zc-protocol/        Shared types: commands, telemetry, device, DTC, shadows, topics
  zc-canbus-tools/    CAN bus / OBD-II diagnostic tools (5 tools, trait-based)
  zc-log-tools/       Multi-format log parsing + 4 analysis tools
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

Supports 4 log formats with auto-detection: syslog (RFC 3164/5424), journald, JSON lines, plaintext.

## Cloud API Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/health` | Health check |
| `GET` | `/api/v1/devices` | List all devices |
| `GET` | `/api/v1/devices/{id}` | Get device details |
| `GET` | `/api/v1/devices/{id}/telemetry` | Get device telemetry |
| `POST` | `/api/v1/commands` | Dispatch a NL command to a device |
| `GET` | `/api/v1/commands` | List recent commands |
| `GET` | `/api/v1/commands/{id}` | Get command status and response |
| `POST` | `/api/v1/commands/{id}/respond` | Ingest command response from device |
| `POST` | `/api/v1/heartbeat` | Ingest device heartbeat |
| `GET` | `/api/v1/ws` | WebSocket for real-time events |

### WebSocket Events

- `command_dispatched` — new command sent to device
- `command_response` — device response received
- `device_heartbeat` — device heartbeat received
- `device_status_changed` — device status transition

## Getting Started

### Prerequisites

- Rust (edition 2024)
- Node.js + pnpm
- PostgreSQL 16 (optional — tests run with in-memory fallback)

### Build & Test

```bash
# Build all crates
cargo build --workspace

# Run all tests (245 tests, no external dependencies required)
cargo test --workspace

# Lint
cargo clippy --workspace -- -D warnings

# Format
cargo fmt --all
cargo fmt --all -- --check   # check only
```

### Run the Cloud API

```bash
# Without database (in-memory mode with sample data)
cargo run -p zc-cloud-api

# With PostgreSQL
DATABASE_URL=postgres://user:pass@localhost/zeroclaw cargo run -p zc-cloud-api

# With Bedrock inference enabled
BEDROCK_ENABLED=true AWS_REGION=us-east-1 cargo run -p zc-cloud-api
```

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
