# IoT Fleet Command Platform — ZeroClaw Remote Diagnostics

## Project Overview

Intelligent command-and-control platform for IoT device fleets (primarily connected vehicles). Combines edge-side AI inference with cloud fallback for remote diagnostics, log analysis, and natural-language device interaction.

**Status**: Phase 1 complete — all 6 Rust crates + Terraform infrastructure

## Architecture (Three Layers)

### Layer 1 — Edge (On-Device)
- **ZeroClaw**: Rust-based AI agent runtime (<5 MB, <10 ms startup, compile-time memory safety)
- **Ollama**: Local LLM inference (Phi-3 Mini / TinyLlama / Gemma 2B, quantized) — handles ~80% of queries at zero API cost
- CAN bus / OBD-II tooling for vehicle ECU diagnostics
- Application log analysis module (syslog, journald, JSON, plaintext)

### Layer 2 — Cloud Orchestration
- **AWS IoT Core**: MQTT messaging, device registry, device shadow, X.509 mutual TLS
- **AWS Bedrock**: Cloud LLM fallback — Nova Lite (default), Claude Haiku/Sonnet (escalation via intelligent prompt routing)
- AWS Lambda for command routing logic, API Gateway for frontend API

### Layer 3 — Frontend
- Web UI: fleet dashboard, device selection, natural-language command interface, real-time response display, audit trail

## Hybrid Inference Strategy

| Tier | Engine | Handles | Latency | Cost |
|------|--------|---------|---------|------|
| Tier 1 | Ollama (local) | Structured commands, known patterns, health checks, log filtering | <100 ms | $0 |
| Tier 2 | Bedrock (cloud) | Complex reasoning, anomaly detection, root-cause analysis | 200–1500 ms | $0.001–$0.015/query |

## PoC Scope
- 10–50 ARM devices (Raspberry Pi 4/5 or industrial SBCs) with CAN bus adapters
- Vehicle diagnostics: DTC retrieval, OBD-II PIDs, real-time sensor readings
- Log analysis: error/warning detection, failure classification, pattern querying
- Natural-language command interface for operators
- Read-only CAN bus mode (no ECU writes until security model validated)

## ZeroClaw Reference

- **Repo**: https://github.com/zeroclaw-labs/zeroclaw
- **Language**: Rust — trait-driven, swappable subsystems (providers, channels, memory, tools, runtime)
- **Binary**: ~8.8 MB, <5 MB RAM, ~10 ms cold startup
- **Build from source**: `cargo build --release --locked`
- **Install**: `cargo install --path . --force --locked` or `brew install zeroclaw`
- **Config**: TOML at `~/.zeroclaw/config.toml`
- **Key crates**: `src/` (core runtime), `crates/robot-kit/` (subsystem components), `web/` (dashboard), `firmware/` (hardware integration)
- **Providers**: OpenAI, Anthropic, OpenRouter, custom endpoints
- **Memory**: SQLite hybrid search (vector + FTS5/BM25), PostgreSQL, Markdown, or none
- **License**: MIT OR Apache-2.0

## Key Technology Choices
- **Rust** — ZeroClaw agent runtime (memory safety, small binary, fast startup)
- **Ollama** — On-device LLM inference (offline capable, zero cost)
- **AWS Bedrock** — Cloud LLM with tiered model selection and prompt caching
- **AWS IoT Core** — MQTT device connectivity, registry, shadows
- **CAN bus / OBD-II** — Vehicle diagnostic protocols
- **X.509 / mTLS** — Per-device certificate authentication

## Security Model
- ZeroClaw: pairing codes, workspace scoping, command allowlisting, localhost binding
- No external skills/plugins — all tooling custom-built and internally audited
- TLS 1.3 everywhere, credentials in AWS Secrets Manager
- CAN bus hardware firewall, ISO 21434 alignment target
- Full command audit trail (CloudTrail + CloudWatch)

## Success Criteria (PoC)
- Local inference p95 latency: <200 ms
- Bedrock fallback p95 latency: <2000 ms
- Local query coverage: >=80% of total queries
- Per-device monthly cost: <$5 (at 50 devices)
- Command success rate: >98%
- Unauthorized command prevention: 100% blocked
- Log analysis accuracy: >95% error/warning detection

## Build / Test / Lint

```bash
# Build all crates
cargo build --workspace

# Build release (optimized for edge devices)
cargo build --profile release-edge -p zc-fleet-agent

# Test all crates
cargo test --workspace

# Test a single crate
cargo test -p zc-canbus-tools
cargo test -p zc-log-tools
cargo test -p zc-mqtt-channel
cargo test -p zc-fleet-agent

# Run a single test
cargo test -p zc-fleet-agent execute_log_tool_succeeds

# Lint
cargo clippy --workspace -- -D warnings

# Format
cargo fmt --all
cargo fmt --all -- --check   # check only
```

## Project Structure

```
crates/
  zc-protocol/       — Shared types: commands, telemetry, device, DTC, shadows, topics
  zc-canbus-tools/   — CAN bus / OBD-II diagnostic tools (5 tools, trait-based)
  zc-log-tools/      — Multi-format log parsing + 4 analysis tools
  zc-mqtt-channel/   — MQTT channel abstraction for AWS IoT Core (mTLS)
  zc-fleet-agent/    — Edge agent binary (wires all crates + MQTT event loop)
  zc-cloud-api/      — Cloud API server (Axum REST API, in-memory state)
infra/
  modules/
    networking/        — VPC, subnets (public/private), NAT, routing
    iot-core/          — Thing types, thing groups, IoT policies, topic rules
    compute/           — Lambda (Rust/AL2023/ARM64), API Gateway HTTP API
    data/              — RDS PostgreSQL 16, Secrets Manager
    monitoring/        — CloudWatch alarms, dashboard
```

### Key Patterns
- **Trait abstractions** for testability: `CanInterface`, `LogSource`, `Channel`
- **Mock implementations** for testing without hardware: `MockCanInterface`, `MockLogSource`, `MockChannel`
- **ToolResult** struct pattern (tool_name, success, data, summary, error) — duplicated in canbus + log crates
- **CanTool / LogTool traits**: name(), description(), parameters_schema(), execute(args, backend)
- **all_tools()** factory functions return `Vec<Box<dyn XxxTool>>`
- Edition 2024 Rust — use `if let ... && let ...` for clippy collapsible_if
- `cargo fmt --all` (not `--workspace`) on this Rust version

## Terraform (Infrastructure)

```bash
cd infra/

# Initialize (first time or after adding modules)
terraform init

# Validate configuration
terraform validate

# Format
terraform fmt -recursive

# Plan (requires AWS credentials)
terraform plan -var-file=terraform.tfvars

# Apply
terraform apply -var-file=terraform.tfvars
```

Copy `infra/terraform.tfvars.example` to `infra/terraform.tfvars` before planning.
