# ZeroClaw Remote Diagnostics — Architecture

## Table of Contents

1. [System Overview](#1-system-overview)
2. [Three-Layer Architecture](#2-three-layer-architecture)
3. [Crate Dependency Graph](#3-crate-dependency-graph)
4. [zc-protocol — Shared Type System](#4-zc-protocol--shared-type-system)
5. [zc-canbus-tools — CAN Bus & OBD-II](#5-zc-canbus-tools--can-bus--obd-ii)
6. [zc-log-tools — Log Analysis](#6-zc-log-tools--log-analysis)
7. [zc-mqtt-channel — MQTT Abstraction](#7-zc-mqtt-channel--mqtt-abstraction)
8. [zc-fleet-agent — Edge Runtime](#8-zc-fleet-agent--edge-runtime)
9. [zc-cloud-api — REST API Server](#9-zc-cloud-api--rest-api-server)
10. [Inference Pipeline](#10-inference-pipeline)
11. [MQTT Topic Schema](#11-mqtt-topic-schema)
12. [End-to-End Data Flows](#12-end-to-end-data-flows)
13. [Frontend Architecture](#13-frontend-architecture)
14. [AWS Infrastructure](#14-aws-infrastructure)
15. [Security Model](#15-security-model)
16. [Key Design Patterns](#16-key-design-patterns)
17. [Performance Targets](#17-performance-targets)

---

## 1. System Overview

ZeroClaw Remote Diagnostics is an intelligent command-and-control platform for IoT device
fleets, purpose-built for connected vehicle diagnostics. Operators type natural-language
commands in a web UI; the platform routes them through a tiered inference pipeline to the
correct edge action — a diagnostic tool, a system command, or a conversational reply.

**Core design goals:**
- **Edge-first**: ~80% of queries handled on-device at zero API cost
- **Offline-capable**: Edge inference + local log analysis work without cloud connectivity
- **Read-only by default**: CAN bus is strictly read-only; no ECU writes until security model is validated
- **Trait-driven**: Every hardware boundary (CAN, logs, MQTT, inference) is abstracted by a trait with a mock implementation, enabling hardware-free testing

**Technology choices:**

| Layer | Technology | Rationale |
|-------|-----------|-----------|
| Edge runtime | Rust (Cargo workspace) | Memory safety, <5 MB binary, ~10 ms cold start |
| On-device LLM | Ollama (phi3:mini) | Offline capable, zero cost, ~50–200 ms inference |
| Cloud LLM fallback | AWS Bedrock (Nova Lite) | Handles ambiguous queries, $0.001/query |
| MQTT broker | AWS IoT Core (mTLS) | Per-device X.509 certs, 128 KB payload limit |
| REST API | Axum 0.8 (Rust) | Shared types with edge via zc-protocol |
| Frontend | SvelteKit 5 + Tailwind 4 | Compiler reactivity, smallest SPA bundle |
| Infrastructure | Terraform (AWS) | IaC from day one, reproducible environments |

---

## 2. Three-Layer Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│  LAYER 3 — FRONTEND                                                      │
│                                                                          │
│  SvelteKit 5 SPA (adapter-static, SSR disabled)                         │
│  ┌────────────────────────────────────────────────────────────────────┐ │
│  │  /              /devices/[id]         /commands                   │ │
│  │  Device list    4-tab detail          Fleet command history        │ │
│  │                 (Overview|Commands    Real-time WS updates         │ │
│  │                  |Shadows|Telemetry)                               │ │
│  └────────────────────────────────────────────────────────────────────┘ │
│  Components: DeviceCard, CommandForm, ShadowPanel, TelemetryPanel,       │
│              SparklineChart, JsonView, ServiceIndicator, StatusBadge     │
│  WebSocket store: auto-reconnect + exponential backoff                   │
└───────────────────────────────┬─────────────────────────────────────────┘
                                │ HTTP REST + WebSocket
                                ▼
┌─────────────────────────────────────────────────────────────────────────┐
│  LAYER 2 — CLOUD ORCHESTRATION                                           │
│                                                                          │
│  ┌──────────────────────────────┐  ┌─────────────────────────────────┐  │
│  │  zc-cloud-api (Axum REST)    │  │  AWS IoT Core                   │  │
│  │                              │  │                                 │  │
│  │  Inference Engine            │  │  Thing registry                 │  │
│  │  ┌──────────────────────┐    │  │  MQTT broker (port 8883, mTLS)  │  │
│  │  │ RuleBasedEngine      │    │  │  Topic rules → CloudWatch       │  │
│  │  │ (pattern matching)   │    │  │  Device shadows (desired/       │  │
│  │  │ ~80% coverage        │◄───┤  │   reported/delta)               │  │
│  │  └──────────────────────┘    │  └─────────────────────────────────┘  │
│  │  ┌──────────────────────┐    │                                        │
│  │  │ BedrockEngine        │    │  ┌─────────────────────────────────┐  │
│  │  │ (Nova Lite via SDK)  │    │  │  AWS Bedrock                    │  │
│  │  │ ambiguous queries    │◄───┤  │  Nova Lite (default)            │  │
│  │  └──────────────────────┘    │  │  Claude Haiku/Sonnet (escalate) │  │
│  │                              │  └─────────────────────────────────┘  │
│  │  REST API (16 endpoints)     │                                        │
│  │  WebSocket /api/v1/ws        │  ┌─────────────────────────────────┐  │
│  │  MQTT Bridge                 │  │  RDS PostgreSQL 16               │  │
│  │  WsEvent broadcast (256)     │  │  (devices, commands, telemetry,  │  │
│  └──────────────────────────────┘  │   heartbeats, shadows)           │  │
│                                    └─────────────────────────────────┘  │
└──────────────────────┬───────────────────────────────────────────────────┘
                       │ MQTT over mTLS (port 8883 prod / 1883 dev)
                       ▼
┌─────────────────────────────────────────────────────────────────────────┐
│  LAYER 1 — EDGE (On-Device, 10–50 ARM devices)                           │
│                                                                          │
│  zc-fleet-agent (Rust binary, ~8.8 MB)                                  │
│  ┌────────────────┐  ┌──────────────┐  ┌────────────────────────────┐   │
│  │ CommandExecutor│  │ ToolRegistry │  │ Shell Executor             │   │
│  │ ActionKind     │  │ 10 tools     │  │ 21-command allowlist       │   │
│  │ routing        │  │ O(1) lookup  │  │ injection detection        │   │
│  └────────────────┘  └──────────────┘  └────────────────────────────┘   │
│  ┌────────────────┐  ┌──────────────┐  ┌────────────────────────────┐   │
│  │ OllamaClient   │  │ zc-canbus-   │  │ zc-log-tools               │   │
│  │ phi3:mini LLM  │  │ tools        │  │ 5 log analysis tools       │   │
│  │ local inference│  │ 5 OBD-II     │  │ syslog/journald/json/text  │   │
│  └────────────────┘  │ tools        │  └────────────────────────────┘   │
│                       └──────────────┘                                   │
│  Background tasks: mqtt_loop + heartbeat (30s) + shadow_sync (60s)       │
│                                                                          │
│  Hardware interfaces:                                                    │
│  CAN bus adapter ──► SocketCanInterface (Phase 2; MockCanInterface now)  │
│  /var/log/syslog ──► FileLogSource                                       │
│  Ollama HTTP API ──► http://localhost:11434                              │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## 3. Crate Dependency Graph

```
zc-e2e-tests
  ├── zc-fleet-agent (lib)
  │     ├── zc-protocol
  │     ├── zc-canbus-tools
  │     ├── zc-log-tools
  │     └── zc-mqtt-channel
  └── zc-cloud-api (lib)
        ├── zc-protocol
        └── zc-mqtt-channel

zc-fleet-agent (bin)
  └── zc-fleet-agent (lib)

zc-cloud-api (bin)
  └── zc-cloud-api (lib)

zc-canbus-tools
  └── zc-protocol      (DtcCode, CanFrame)

zc-log-tools
  └── (no zc-protocol dependency — uses its own LogEntry/ToolResult)

zc-mqtt-channel
  └── zc-protocol      (CommandEnvelope, CommandResponse, Heartbeat,
                         TelemetryReading, ShadowDelta, ShadowUpdate,
                         ShadowState, DeviceShadowState)

zc-protocol            (no internal deps — shared foundation)
```

**Workspace configuration** (`Cargo.toml`):
- Edition 2024 (Rust latest stable)
- Shared workspace dependencies: tokio 1.42, serde, serde_json, chrono, uuid (v7), sqlx 0.8, axum 0.8, rumqttc 0.24, aws-sdk-bedrockruntime 1.0, socketcan 3.5, reqwest 0.12, thiserror 2.0, tracing 0.1
- `release-edge` profile: `-O s` (optimize binary size for ARM edge devices)
- `release` profile: LTO + single codegen unit + strip symbols + abort on panic

---

## 4. zc-protocol — Shared Type System

The foundation crate. Zero business logic — pure shared data structures serialized with serde. All crates depend on this.

### Commands

```rust
// Sent cloud → device over MQTT
pub struct CommandEnvelope {
    pub id: Uuid,                           // UUIDv7 (time-sortable)
    pub fleet_id: String,
    pub device_id: String,
    pub natural_language: String,           // Original operator text
    pub parsed_intent: Option<ParsedIntent>,// Pre-classified by cloud inference
    pub correlation_id: Uuid,              // Pairs with CommandResponse
    pub initiated_by: String,              // Operator name/email
    pub created_at: DateTime<Utc>,
    pub timeout_secs: u32,                 // Default: 30
}

// Extracted by any inference engine
pub struct ParsedIntent {
    pub action: ActionKind,
    pub tool_name: String,                 // Tool name OR shell command string
    pub tool_args: serde_json::Value,      // {"pid": "0x0C"} or {"message": "..."}
    pub confidence: f64,                   // 0.0–1.0
}

pub enum ActionKind { Tool, Shell, Reply }

// Sent device → cloud over MQTT
pub struct CommandResponse {
    pub command_id: Uuid,
    pub correlation_id: Uuid,
    pub device_id: String,
    pub status: CommandStatus,             // Pending|Sent|Processing|Completed|Failed|Timeout|Cancelled
    pub inference_tier: InferenceTier,     // Local|CloudLite|CloudHaiku|CloudSonnet
    pub response_text: Option<String>,     // Human-readable summary
    pub response_data: Option<serde_json::Value>,  // Structured tool output
    pub latency_ms: u64,
    pub responded_at: DateTime<Utc>,
    pub error: Option<String>,
}
```

### Devices

```rust
pub struct DeviceInfo {
    pub id: Uuid,
    pub fleet_id: FleetId,
    pub device_id: String,                 // IoT Core thing name
    pub status: DeviceStatus,              // Provisioning|Online|Offline|Maintenance|Decommissioned
    pub vin: Option<String>,
    pub hardware_type: HardwareType,       // RaspberryPi4|RaspberryPi5|IndustrialSbc|Custom(String)
    pub certificate_id: Option<String>,   // X.509 thumbprint for mTLS
    pub last_heartbeat: Option<DateTime<Utc>>,
    pub metadata: serde_json::Value,       // Fleet, firmware version, location, etc.
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub struct Heartbeat {
    pub device_id: String,
    pub fleet_id: String,
    pub status: DeviceStatus,
    pub uptime_secs: u64,
    pub ollama_status: ServiceStatus,      // Running|Stopped|Error|Unknown
    pub can_status: ServiceStatus,
    pub agent_version: String,
    pub timestamp: DateTime<Utc>,
}
```

### Telemetry

```rust
pub struct TelemetryReading {
    pub device_id: String,
    pub time: DateTime<Utc>,
    pub metric_name: String,               // "engine_rpm", "coolant_temp", "cpu_usage"
    pub value_numeric: Option<f64>,
    pub value_text: Option<String>,
    pub value_json: Option<serde_json::Value>,
    pub unit: Option<String>,              // "rpm", "celsius", "percent"
    pub source: TelemetrySource,           // Obd2 | System | Canbus
}

pub struct SystemMetrics {
    pub cpu_percent: f64,
    pub memory_used_bytes: u64,
    pub memory_total_bytes: u64,
    pub disk_percent: f64,
    pub uptime_secs: u64,
    pub cpu_temp_celsius: Option<f64>,
    pub ollama_running: bool,
    pub can_interface_up: bool,
}
```

### DTC (Diagnostic Trouble Codes)

```rust
pub struct DtcCode {
    pub code: String,                      // "P0300", "C0035"
    pub category: DtcCategory,             // Powertrain|Chassis|Body|Network
    pub severity: DtcSeverity,             // Info|Warning|Critical|Unknown
    pub description: Option<String>,       // From compile-time ~5000-code database
    pub mil_status: bool,                  // Check engine light on?
    pub freeze_frame: Option<FreezeFrame>, // Engine state at fault time
}
```

### Device Shadows (AWS IoT inspired)

```rust
pub struct ShadowState {
    pub reported: serde_json::Value,       // Last-known device state
    pub desired: serde_json::Value,        // Cloud-desired target state
    pub version: u64,                      // Monotonically increasing
    pub last_updated: DateTime<Utc>,
}

pub struct ShadowDelta {
    pub device_id: String,
    pub shadow_name: String,               // e.g., "config", "diagnostics", "state"
    pub delta: serde_json::Value,          // Keys where desired ≠ reported
    pub version: u64,
    pub timestamp: DateTime<Utc>,
}
```

---

## 5. zc-canbus-tools — CAN Bus & OBD-II

### Trait Architecture

```
CanInterface (trait)
    ├── SocketCanInterface    — Linux socketcan, real hardware (Phase 2)
    └── MockCanInterface      — Test double (simulates timeouts in local dev)

CanTool (trait)
    ├── ReadPid              — OBD-II PID sensor reads
    ├── ReadDtcs             — Diagnostic trouble codes
    ├── ReadVin              — Vehicle Identification Number
    ├── ReadFreeze           — Freeze frame data
    └── CanMonitor           — Raw CAN bus traffic capture
```

### CanInterface Trait

```rust
#[async_trait]
pub trait CanInterface: Send + Sync {
    async fn send_frame(&self, frame: &CanFrame) -> CanResult<()>;
    async fn recv_frame(&self, timeout: Duration) -> CanResult<CanFrame>;
    async fn obd_request(&self, mode: u8, pid: Option<u8>, timeout: Duration) -> CanResult<Vec<CanFrame>>;
}

pub struct CanFrame {
    pub id: u32,                           // 0x7DF = OBD broadcast request
    pub data: Vec<u8>,                     // ≤8 bytes (standard CAN)
}
```

**Safety**: OBD-II modes 2, 5, 10, 14 are blocked (write modes). Only modes 1, 3, 4, 6, 9 (read-only) are permitted. ISO-TP flow control frames (`0x30`) are allowed to pass through.

### 5 Tools

| Tool | Name | Args | Protocol | Returns |
|------|------|------|----------|---------|
| ReadPid | `read_pid` | `{"pid": "0x0C"}` | OBD-II mode 0x01 | Sensor value + unit |
| ReadDtcs | `read_dtcs` | `{}` | OBD-II mode 0x03 | Array of DtcCode |
| ReadVin | `read_vin` | `{}` | OBD-II mode 0x09 PID 0x02, ISO-TP multi-frame | 17-char VIN string |
| ReadFreeze | `read_freeze` | `{}` | OBD-II mode 0x02 | FreezeFrame struct |
| CanMonitor | `can_monitor` | `{"duration_secs": 10}` | Raw CAN receive loop | Array of timestamped frames |

**Common PIDs:**

| PID | Signal | Formula |
|-----|--------|---------|
| 0x0C | Engine RPM | `(A*256 + B) / 4` |
| 0x0D | Vehicle speed km/h | `A` |
| 0x05 | Coolant temperature | `A - 40` °C |
| 0x11 | Throttle position | `A * 100 / 255` % |
| 0x2F | Fuel level | `A * 100 / 255` % |
| 0x04 | Calculated engine load | `A * 100 / 255` % |
| 0x0F | Intake air temperature | `A - 40` °C |
| 0x0E | Timing advance | `A/2 - 64` °crankshaft |

### DTC Database

~5000 DTC codes loaded at compile time from `dtc_db.rs`. Lookup by code string (e.g., "P0300" → "Random/Multiple Cylinder Misfire Detected", severity: Critical). `decode_dtc_bytes()` returns `None` for `0x00/0x00` padding bytes.

---

## 6. zc-log-tools — Log Analysis

### Trait Architecture

```
LogSource (trait)
    ├── FileLogSource    — Real filesystem (/var/log/syslog etc.)
    └── MockLogSource    — Test double with scripted data

LogTool (trait)
    ├── SearchLogs       — Regex search
    ├── AnalyzeErrors    — Error pattern classification
    ├── LogStats         — Statistics and severity distribution
    ├── TailLogs         — Last N lines
    └── QueryJournal     — systemd journal subprocess
```

### LogEntry & Severity

```rust
pub struct LogEntry {
    pub timestamp: Option<DateTime<Utc>>,
    pub severity: LogSeverity,
    pub source: Option<String>,
    pub message: String,
}

// Ordered: Debug < Info < Notice < Warning < Error < Critical
pub enum LogSeverity { Debug, Info, Notice, Warning, Error, Critical, Unknown }
```

### Format Auto-Detection

`parsers::detect_format(lines)` checks the first several lines:

| Format | Detection signal | Example |
|--------|-----------------|---------|
| Syslog RFC 3164 | `"Jan 15 10:30:45 host svc[pid]:"` | `/var/log/syslog` |
| Syslog RFC 5424 | `"<134>2024-01-15T10:30:45Z host svc:"` | systemd-journald via syslog |
| Journald export | `"__REALTIME_TIMESTAMP=..."` key-value pairs | `journalctl --output=export` |
| NDJSON | Lines parse as valid JSON objects | Structured app logs |
| Plaintext | Fallback | All other formats |

### 5 Tools

| Tool | Name | Args | Backend |
|------|------|------|---------|
| SearchLogs | `search_logs` | `{"path": "/var/log/syslog", "query": "error"}` | LogSource + regex |
| AnalyzeErrors | `analyze_errors` | `{"path": "/var/log/syslog"}` | LogSource + 9 pattern categories |
| LogStats | `log_stats` | `{"path": "/var/log/syslog"}` | LogSource + count by severity |
| TailLogs | `tail_logs` | `{"path": "/var/log/syslog", "lines": 50}` | LogSource.tail_lines() |
| QueryJournal | `query_journal` | `{"unit": "nginx.service", "lines": 50}` | `journalctl` subprocess |

**Error categories for `analyze_errors`** (9 total):
Connection, Permission, Resource (memory/disk), Service (segfault/panic), File (ENOENT), DNS (NXDOMAIN), Process (oom-killer), Timeout, CAN bus

**QueryJournal safety**: Unit name validated against `[a-zA-Z0-9.@\-_]+`, 64 KB output cap, 5 s subprocess timeout.

---

## 7. zc-mqtt-channel — MQTT Abstraction

### Channel Trait

```rust
#[async_trait]
pub trait Channel: Send + Sync {
    async fn publish(&self, topic: &str, payload: &[u8], qos: QoS) -> MqttResult<()>;
    async fn subscribe(&self, topic: &str, qos: QoS) -> MqttResult<()>;
}
```

Implementations: `MqttChannel` (real, rumqttc), `MockChannel` (test double).

### MqttChannel

Two constructors:
- `MqttChannel::new(config, fleet_id, device_id)` — mTLS, reads X.509 certs from config paths. Used in production (AWS IoT Core port 8883).
- `MqttChannel::new_plaintext(broker_host, broker_port, client_id, fleet_id, device_id)` — no TLS. Used in local dev (Mosquitto port 1883).

Both return `(MqttChannel, rumqttc::EventLoop)`. The caller drives the eventloop in its own task.

**Packet size configuration:**

| Limit | Value | Owner |
|-------|-------|-------|
| rumqttc `max_packet_size` | 256 KB | Client-side receive buffer |
| AWS IoT Core actual limit | 128 KB | Broker enforced |
| `MAX_MQTT_PAYLOAD` (fleet agent) | 128 KB | Code-level cap before publish |

### Typed Publish / Subscribe Helpers

**Device-level** (fleet agent publishes, cloud subscribes via wildcard):

```
channel.publish_response(response)   → fleet/{fleet_id}/{device_id}/command/response
channel.publish_telemetry(reading)   → fleet/{fleet_id}/{device_id}/telemetry/{source}
channel.publish_heartbeat(hb)        → fleet/{fleet_id}/{device_id}/heartbeat/ping
channel.publish_ack(ack)             → fleet/{fleet_id}/{device_id}/command/ack
```

**Fleet-level** (cloud subscribes to all devices in a fleet):

```
subscribe_fleet_responses(fleet_id)       → fleet/{fleet_id}/+/command/response
subscribe_fleet_heartbeats(fleet_id)      → fleet/{fleet_id}/+/heartbeat/ping
subscribe_fleet_telemetry(fleet_id, src)  → fleet/{fleet_id}/+/telemetry/{source}
subscribe_fleet_shadow_updates(fleet_id)  → fleet/{fleet_id}/+/shadow/update
```

### IncomingMessage Classification

`handler::classify(publish)` parses the topic and returns:

```rust
pub enum IncomingMessage {
    Command(CommandEnvelope),              // command/request → parse envelope
    ShadowDelta(ShadowDelta),              // shadow/delta → parse delta
    ConfigUpdate(serde_json::Value),       // config/update → raw JSON
    Unknown { topic, payload },            // everything else
}
```

### ShadowClient

Generic wrapper over any `Channel` for shadow-specific operations:

```rust
pub struct ShadowClient<'a, C: Channel> { channel, fleet_id, device_id }

shadow_client.report_state(state)   // publish_update to shadow/update
shadow_client.subscribe_delta()     // subscribe to shadow/delta
```

---

## 8. zc-fleet-agent — Edge Runtime

### Startup Sequence

```
main.rs
  1. Load AgentConfig from TOML file
  2. ToolRegistry::with_defaults()        → 10 tools indexed by name
  3. MqttChannel::new() or new_plaintext()
  4. subscribe_commands()                 → command/request + broadcast/commands
     subscribe_shadow_delta()             → shadow/delta
     subscribe_config()                   → config/update
  5. OllamaClient::new() if config.ollama.enabled
  6. MockCanInterface (real: SocketCanInterface in Phase 2)
  7. FileLogSource
  8. SharedShadowState = Arc<RwLock<DeviceShadowState>>
  9. tokio::select! {
       mqtt_loop::run(...)    ← command dispatch (runs forever)
       heartbeat::run(...)    ← every 30s
       shadow_sync::run(...)  ← every 60s
       ctrl_c                 ← graceful shutdown
     }
```

### AgentConfig (TOML)

```toml
fleet_id = "local-fleet"
device_id = "dev-001"
heartbeat_interval_secs = 10     # default: 30
shadow_sync_interval_secs = 30   # default: 60
log_paths = ["/var/log/syslog"]

[mqtt]
broker_host = "localhost"
broker_port = 1883
client_id = "dev-001"
use_tls = false
# ca_cert / client_cert / client_key — required when use_tls = true

[ollama]
host = "http://localhost:11434"
model = "phi3:mini"
timeout_secs = 10
enabled = true
```

### CommandExecutor

The heart of the edge runtime. Processes each `CommandEnvelope`:

```
CommandEnvelope received
        │
        ▼
ParsedIntent present in envelope?
    YES ──► use it directly (cloud already parsed)
    NO  ──► OllamaClient.parse(natural_language)
              SUCCESS ──► use Ollama intent
              FAIL    ──► return error CommandResponse
        │
        ▼
Route on ActionKind:
    Tool  ──► ToolRegistry.lookup(tool_name)
                CanBus ──► execute_can(args, &can_interface)
                Log    ──► execute_log(args, &log_source)
    Shell ──► sanitize_shell_command(tool_name)   ← strip metacharacters
              shell::execute(sanitized_command)
    Reply ──► extract tool_args["message"]
        │
        ▼
Build CommandResponse { status, response_text, response_data, latency_ms, error }
Update SharedShadowState { last_command_id, last_command_tool, last_command_at }
Publish CommandResponse via MQTT
```

### ToolRegistry

O(1) lookup over 10 tools:

```rust
pub struct ToolRegistry {
    can_tools: Vec<Box<dyn CanTool>>,     // index 0–4
    log_tools: Vec<Box<dyn LogTool>>,     // index 0–4
    index: HashMap<String, (ToolKind, usize)>,
}
```

Tool roster:

| Kind | Name | Backed by |
|------|------|-----------|
| CAN | `read_pid` | CanInterface + obd.rs decode |
| CAN | `read_dtcs` | CanInterface + dtc_db lookup |
| CAN | `read_vin` | CanInterface + ISO-TP |
| CAN | `read_freeze` | CanInterface |
| CAN | `can_monitor` | CanInterface recv loop |
| Log | `search_logs` | LogSource + regex |
| Log | `analyze_errors` | LogSource + pattern matching |
| Log | `log_stats` | LogSource + severity count |
| Log | `tail_logs` | LogSource.tail_lines() |
| Log | `query_journal` | journalctl subprocess |

### Shell Executor Safety Layers

Five independent checks before execution:

```
Input command string
        │
        ▼
1. Shell metacharacter scan   (;, |, `, $(), >, <, &&, ||, \n, \r)
        │ FAIL → ShellError::Injection
        ▼
2. shell_words::split()       (safe tokenization, no shell interpretation)
        │
        ▼
3. Blocklist check            (rm, dd, sudo, curl, wget, bash, ssh, reboot, ...)
        │ MATCH → ShellError::Blocked
        ▼
4. Allowlist check            (cat, ls, df, free, uname, uptime, ps, ip, ...)
        │ NO MATCH → ShellError::NotAllowed
        ▼
5. Sensitive path check       (/etc/shadow, /root, /.ssh, .env, credentials)
        │ MATCH → ShellError::SensitivePath
        ▼
tokio::process::Command::new(program).args(args)
  with 5s timeout and 8KB output cap
```

`systemctl` is further restricted to read-only subcommands: `status`, `is-active`, `is-enabled`, `list-units`, `show`.

### Background Tasks

**heartbeat::run()**: Publishes `Heartbeat` every 30 s (configurable). Includes uptime, Ollama service status, CAN interface status, agent version.

**shadow_sync::run()**: Publishes `ShadowUpdate` (via `ShadowClient::report_state`) every 60 s. Payload includes tool count, service statuses, last command metadata. Cloud processes update, computes delta vs. desired, publishes `ShadowDelta` back if non-empty.

---

## 9. zc-cloud-api — REST API Server

### AppState

Dual-mode: database-backed (production) or in-memory (tests, local dev without `DATABASE_URL`).

```rust
pub struct AppState {
    pub pool: Option<PgPool>,
    pub devices: Arc<RwLock<HashMap<String, DeviceInfo>>>,
    pub commands: Arc<RwLock<Vec<CommandRecord>>>,
    pub event_tx: broadcast::Sender<WsEvent>,          // capacity: 256
    pub inference: Arc<dyn InferenceEngine>,
    pub mqtt: Option<Arc<dyn Channel>>,
    pub shadows: Arc<RwLock<HashMap<(String, String), ShadowState>>>,
}
```

Constructors:
- `AppState::new()` — in-memory, RuleBasedEngine, no MQTT
- `AppState::with_sample_data()` — pre-populates 3 sample devices (rpi-001, rpi-002, sbc-010)
- `AppState::with_sample_data_and_inference(engine)` — custom inference engine (for Bedrock)
- `AppState::with_pool(pool, inference)` — database-backed production mode

### REST API Endpoints

| Method | Path | Description | Response |
|--------|------|-------------|----------|
| GET | `/health` | Health check | `{"status":"ok","version":"0.1.0"}` |
| GET | `/api/v1/devices` | List all devices | `Vec<DeviceSummary>` |
| POST | `/api/v1/devices` | Provision a device | `201 DeviceInfo` / `409 Conflict` |
| GET | `/api/v1/devices/{id}` | Get device detail | `DeviceDetail` |
| GET | `/api/v1/commands` | List all commands | `Vec<Command>` |
| POST | `/api/v1/commands` | Send NL command | `Command` with ParsedIntent |
| GET | `/api/v1/commands/{id}` | Get command + response | `Command` |
| POST | `/api/v1/commands/{id}/respond` | Ingest device response | `200` |
| GET | `/api/v1/devices/{id}/telemetry` | Get telemetry readings | `Vec<TelemetryReading>` |
| POST | `/api/v1/devices/{id}/telemetry` | Ingest telemetry batch | `{"status":"ok","count":N}` |
| GET | `/api/v1/devices/{id}/shadows` | List all shadows | `Vec<ShadowSummary>` |
| GET | `/api/v1/devices/{id}/shadows/{name}` | Get shadow (reported + desired + delta) | `ShadowResponse` |
| PUT | `/api/v1/devices/{id}/shadows/{name}/desired` | Set desired state | `200` |
| POST | `/api/v1/heartbeat` | Ingest device heartbeat | `200` |
| GET | `/api/v1/ws` | WebSocket upgrade | Persistent WS connection |

**Middleware**: CORS (allow all origins), gzip compression, structured tracing.

### send_command Flow

```
POST /api/v1/commands
      │
      ▼
1. Validate device exists (404 if not)
2. inference.parse(command_text)
      RuleBasedEngine: substring match → ParsedIntent
      BedrockEngine:   AWS Converse API → ParsedIntent
3. Create CommandEnvelope { id: UUIDv7, parsed_intent, ... }
4. Store CommandRecord in memory/DB
5. Broadcast WsEvent::CommandDispatched
6. If mqtt is Some: publish envelope to MQTT
      topic: fleet/{fleet_id}/{device_id}/command/request
7. Return Command { id, status: "sent", inference_tier, ... }
```

### MQTT Bridge

Runs as a background task alongside the Axum server. Receives all fleet-level MQTT
messages and dispatches them back into the AppState:

```
mqtt_bridge::run(eventloop, state)
      │
      ▼  (for each incoming MQTT Publish)
classify topic →
    command/response   → ingest_response(payload, &state)
                          → update CommandRecord + broadcast CommandResponse WsEvent
    heartbeat/ping     → ingest_heartbeat(payload, &state)
                          → update device.last_heartbeat + broadcast DeviceHeartbeat
    telemetry/*        → ingest_telemetry(payload, &state)
                          → store readings + broadcast TelemetryIngested
    shadow/update      → handle_shadow_update(payload, &state)
                          → upsert reported (JSONB merge), compute delta,
                            publish ShadowDelta to MQTT if non-empty,
                            broadcast ShadowUpdated WsEvent
```

`compute_delta(desired, reported)`: Returns a JSON object containing only the keys in
`desired` whose values differ from `reported`. Empty object → no delta published.

### WebSocket Events

```rust
pub enum WsEvent {
    CommandDispatched  { command_id, device_id, command, initiated_by, created_at },
    CommandResponse    { command_id, device_id, status, inference_tier,
                         response_text, response_data, error, latency_ms, responded_at },
    DeviceHeartbeat    { device_id, timestamp },
    DeviceStatusChanged { device_id, old_status, new_status, changed_at },
    DeviceProvisioned  { device_id, fleet_id, hardware_type, provisioned_at },
    TelemetryIngested  { device_id, count, source, timestamp },
    ShadowUpdated      { device_id, shadow_name, version, timestamp },
}
```

Serialized with `#[serde(tag = "type", rename_all = "snake_case")]` — each event has a
`"type"` discriminator field for frontend pattern matching.

### Database Schema (5 migrations)

| Table | Key columns | Notes |
|-------|------------|-------|
| `devices` | device_id, fleet_id, status, vin, hardware_type, certificate_id, last_heartbeat, metadata (JSONB) | |
| `commands` | id (UUIDv7), device_id, natural_language, parsed_intent (JSONB), status, inference_tier, response_text, response_data (JSONB), latency_ms | |
| `telemetry_readings` | device_id, time, metric_name, value_numeric, value_text, value_json (JSONB), unit, source | TimescaleDB candidate |
| `heartbeats` | device_id, uptime_secs, ollama_status, can_status, agent_version, timestamp | |
| `device_shadows` | device_id, shadow_name, reported (JSONB), desired (JSONB), version, last_updated | JSONB `\|\|` merge for reported |

---

## 10. Inference Pipeline

Three inference layers process the same natural-language text. Only one cloud-side engine
is active at a time, controlled by `INFERENCE_ENGINE` environment variable.

```
Operator types: "is the powertrain healthy?"
                          │
                          ▼
              ┌───────────────────────────┐
              │  Cloud API — Inference    │
              │                           │
              │  INFERENCE_ENGINE=local   │   INFERENCE_ENGINE=bedrock
              │  ┌─────────────────────┐  │   ┌─────────────────────┐
              │  │  RuleBasedEngine    │  │   │  BedrockEngine      │
              │  │  Substring match    │  │   │  AWS Converse API   │
              │  │  40+ patterns       │  │   │  Nova Lite (LLM)    │
              │  │  <1 ms, $0          │  │   │  200–1500 ms        │
              │  │  ~80% coverage      │  │   │  ~$0.001/query      │
              │  └─────────────────────┘  │   └─────────────────────┘
              └───────────┬───────────────┘
                          │ ParsedIntent (or None)
                          ▼
              CommandEnvelope { parsed_intent: Some(...) }
                          │
                    MQTT publish
                          │
                          ▼
              ┌───────────────────────────┐
              │  Fleet Agent — Executor   │
              │                           │
              │  parsed_intent present?   │
              │  YES: use it directly     │
              │  NO:  ┌─────────────────┐ │
              │       │  OllamaClient   │ │
              │       │  phi3:mini      │ │
              │       │  50–500 ms, $0  │ │
              │       └─────────────────┘ │
              └───────────┬───────────────┘
                          │ ActionKind routed
                    Tool / Shell / Reply
```

### RuleBasedEngine Patterns

The engine uses case-insensitive substring matching. If the operator's phrase contains
any of the listed substrings, the engine returns the corresponding `ParsedIntent`.

**CAN / OBD-II tools:**

| Triggers (any substring) | → Tool |
|--------------------------|--------|
| "read dtc", "get dtc", "trouble code", "engine code", "check code", "fault code" | `read_dtcs` |
| "read vin", "get vin", "vehicle identification", "show vin", "what is the vin" | `read_vin` |
| "freeze frame", "freeze data", "snapshot data", "read freeze" | `read_freeze` |
| "monitor can", "sniff can", "capture can", "can bus traffic", "can traffic" | `can_monitor` |
| ("rpm"/"engine speed") + verb | `read_pid` pid=0x0C |
| ("speed"/"vehicle speed") + verb | `read_pid` pid=0x0D |
| ("coolant"/"engine temp") + verb | `read_pid` pid=0x05 |
| ("fuel level"/"fuel") + verb | `read_pid` pid=0x2F |

**Log tools:**

| Triggers | → Tool |
|----------|--------|
| "search log", "grep log", "find in log", "search for" | `search_logs` |
| "analyze error", "error analysis", "what error", "find error" | `analyze_errors` |
| "log stat", "log summar", "log overview", "show stat" | `log_stats` |
| "tail log", "recent log", "latest log", "show log", "last log" | `tail_logs` |
| "journal for", "journalctl", "service log", "systemd log" | `query_journal` |

**Shell commands:**

| Triggers | → Shell command |
|----------|-----------------|
| "ip address", "ip addr", "network interface" | `ip -brief addr` |
| "cpu temp", "cpu temperature", "processor temp" | `cat /sys/class/thermal/thermal_zone0/temp` |
| "gpu temp", "gpu temperature" | `vcgencmd measure_temp` |
| "disk space", "disk usage", "storage", "free space" | `df -h` |
| "memory", "ram", "free mem" | `free -h` |
| "uptime" | `uptime` |
| "kernel version", "kernel", "uname" | `uname -a` |
| "cpu info", "processor info", "lscpu" | `lscpu` |
| "process", "running process" | `ps aux` |
| "hostname" | `hostname` |

Phrases not matching any pattern return `None` from the rule engine. With `INFERENCE_ENGINE=bedrock`, Bedrock handles these. With `INFERENCE_ENGINE=local`, the `CommandEnvelope` is sent without a `parsed_intent` and Ollama handles it on-device.

### BedrockEngine

Uses the AWS SDK `bedrockruntime::converse()` API. Sends a system prompt describing all 10 tools + 10 shell patterns + reply action. The LLM responds with a JSON object:

```json
{"action": "tool", "tool_name": "read_dtcs", "tool_args": {}, "confidence": 0.85}
```

`extract_json()` helper handles models that wrap JSON in markdown code fences. A 15 s timeout wraps the SDK call (cold starts can take 8–10 s).

### Ollama (On-Device)

`OllamaClient` calls `POST http://localhost:11434/api/chat` with `format: "json"` and `stream: false`. Returns a `ChatResponse` with a `message.content` JSON string. Validates the JSON against three action types:

- `tool`: tool_name must be in `KNOWN_TOOLS`, confidence ≥ 0.3
- `shell`: command field must be non-empty; sanitized with `sanitize_shell_command()`
- `reply`: message field must be non-empty

Graceful fallbacks handle phi3:mini quirks: if `action` field contains a tool name directly (phi3 sometimes does this), it's treated as `action=tool`.

---

## 11. MQTT Topic Schema

All topics follow the pattern `fleet/{fleet_id}/{device_id}/{category}/{action}`.

```
Direction   Topic                                              Payload type
──────────  ─────────────────────────────────────────────────  ──────────────────────
Cloud → Device:
  PUBLISH   fleet/{fleet_id}/{device_id}/command/request       CommandEnvelope (JSON)
  PUBLISH   fleet/{fleet_id}/{device_id}/shadow/delta          ShadowDelta (JSON)
  PUBLISH   fleet/{fleet_id}/broadcast/command/request         CommandEnvelope (JSON)
  PUBLISH   fleet/{fleet_id}/broadcast/config/update           Config JSON

Device → Cloud:
  PUBLISH   fleet/{fleet_id}/{device_id}/command/response      CommandResponse (JSON)
  PUBLISH   fleet/{fleet_id}/{device_id}/command/ack           Ack JSON
  PUBLISH   fleet/{fleet_id}/{device_id}/heartbeat/ping        Heartbeat (JSON)
  PUBLISH   fleet/{fleet_id}/{device_id}/shadow/update         ShadowUpdate (JSON)
  PUBLISH   fleet/{fleet_id}/{device_id}/telemetry/obd2        TelemetryReading (JSON)
  PUBLISH   fleet/{fleet_id}/{device_id}/telemetry/system      SystemMetrics (JSON)
  PUBLISH   fleet/{fleet_id}/{device_id}/telemetry/canbus      Raw CAN telemetry (JSON)
  PUBLISH   fleet/{fleet_id}/{device_id}/alert/notify          Alert (JSON)

Cloud subscriptions (wildcard, catches all devices in fleet):
  SUBSCRIBE fleet/{fleet_id}/+/command/response
  SUBSCRIBE fleet/{fleet_id}/+/heartbeat/ping
  SUBSCRIBE fleet/{fleet_id}/+/telemetry/#
  SUBSCRIBE fleet/{fleet_id}/+/shadow/update

Device subscriptions (per-device):
  SUBSCRIBE fleet/{fleet_id}/{device_id}/command/request
  SUBSCRIBE fleet/{fleet_id}/broadcast/command/request
  SUBSCRIBE fleet/{fleet_id}/{device_id}/shadow/delta
  SUBSCRIBE fleet/{fleet_id}/{device_id}/config/update
```

**QoS**: Commands use QoS 1 (at-least-once). Heartbeats and telemetry use QoS 0 (fire-and-forget).

---

## 12. End-to-End Data Flows

### A. Command Dispatch and Response

```
Browser                Cloud API           MQTT Broker        Fleet Agent
  │                        │                    │                   │
  │ POST /api/v1/commands  │                    │                   │
  │ {"command":"disk space"}│                   │                   │
  ├───────────────────────►│                    │                   │
  │                        │ inference.parse()  │                   │
  │                        │ → ParsedIntent     │                   │
  │                        │   {action:Shell,   │                   │
  │                        │    tool_name:"df -h│                   │
  │                        │    confidence:0.95}│                   │
  │                        │                    │                   │
  │                        │ store CommandRecord│                   │
  │                        │ broadcast CommandDispatched (WS)       │
  │ 200 Command{id,status} │                    │                   │
  │◄───────────────────────┤                    │                   │
  │                        │ MQTT publish       │                   │
  │                        │ CommandEnvelope    │                   │
  │                        ├───────────────────►│                   │
  │                        │                    │ deliver to device │
  │                        │                    ├──────────────────►│
  │                        │                    │                   │ classify()
  │                        │                    │                   │ → Command
  │                        │                    │                   │
  │                        │                    │                   │ executor.execute()
  │                        │                    │                   │ → ActionKind::Shell
  │                        │                    │                   │ → shell::execute("df -h")
  │                        │                    │                   │ ← ShellResult{stdout}
  │                        │                    │                   │
  │                        │                    │  MQTT publish     │
  │                        │                    │  CommandResponse  │
  │                        │                    │◄──────────────────┤
  │                        │ mqtt_bridge        │                   │
  │                        │ handle_incoming()  │                   │
  │                        │◄───────────────────┤                   │
  │                        │ update CommandRecord                   │
  │                        │ broadcast CommandResponse (WS)         │
  │ WS: CommandResponse    │                    │                   │
  │◄───────────────────────┤                    │                   │
```

### B. Heartbeat Flow

```
Fleet Agent                   MQTT Broker            Cloud API
     │                              │                     │
     │  every 30s                   │                     │
     │  publish_heartbeat()         │                     │
     │  Heartbeat{device_id,        │                     │
     │    uptime, statuses, ...}    │                     │
     ├─────────────────────────────►│                     │
     │                              │  fleet/+/heartbeat  │
     │                              ├────────────────────►│
     │                              │                     │ mqtt_bridge
     │                              │                     │ ingest_heartbeat()
     │                              │                     │ update device.last_heartbeat
     │                              │                     │ broadcast DeviceHeartbeat (WS)
     │                              │                     │
     │                              │             Browser (WS) ◄── DeviceHeartbeat event
```

### C. Shadow Sync Flow

```
Fleet Agent                   MQTT Broker            Cloud API            Browser
     │                              │                     │                   │
     │  every 60s                   │                     │                   │
     │  shadow_client.report_state()│                     │                   │
     │  → ShadowUpdate{reported:{   │                     │                   │
     │      tool_count:10,          │                     │                   │
     │      can_status:"mock", ...}}│                     │                   │
     ├─────────────────────────────►│                     │                   │
     │                              │  fleet/+/shadow/update                  │
     │                              ├────────────────────►│                   │
     │                              │                     │ handle_shadow_update()
     │                              │                     │ upsert reported (JSONB merge)
     │                              │                     │ compute_delta(desired, reported)
     │                              │                     │ if delta non-empty:
     │                              │   publish ShadowDelta│                   │
     │                              │◄────────────────────┤                   │
     │  receive ShadowDelta         │                     │                   │
     │◄─────────────────────────────┤                     │ broadcast ShadowUpdated (WS)
     │  mqtt_loop: handle_shadow_delta                    ├──────────────────►│
     │  merge desired into reported │                     │                   │
     │  report_state() (ack)        │                     │                   │
     ├─────────────────────────────►│                     │                   │
```

### D. Device Provisioning

```
Browser                Cloud API             MQTT Broker        Fleet Agent
  │                        │                      │                  │
  │ POST /api/v1/devices   │                      │                  │
  │ {device_id, fleet_id,  │                      │                  │
  │  hardware_type, vin}   │                      │                  │
  ├───────────────────────►│                      │                  │
  │ 201 DeviceInfo         │                      │                  │
  │ {status:"provisioning"}│                      │                  │
  │◄───────────────────────┤                      │                  │
  │                        │                      │                  │
  │                        │                      │  (agent already  │
  │                        │                      │   running with   │
  │                        │                      │   device config) │
  │                        │                      │                  │
  │                        │             30s later: heartbeat        │
  │                        │◄─────────────────────────────────────── │
  │                        │ update status "online"                   │
  │                        │ broadcast DeviceStatusChanged (WS)       │
  │ WS: DeviceStatusChanged│                      │                  │
  │◄───────────────────────┤                      │                  │
```

---

## 13. Frontend Architecture

### SPA Structure

```
frontend/
├── src/
│   ├── routes/
│   │   ├── +layout.svelte          ← WebSocket connect, nav, Live indicator
│   │   ├── +page.svelte            ← /   Device list
│   │   ├── devices/[id]/
│   │   │   └── +page.svelte        ← /devices/:id   4-tab device detail
│   │   └── commands/
│   │       └── +page.svelte        ← /commands      Fleet command history
│   └── lib/
│       ├── types/
│       │   ├── index.ts            ← WsEvent discriminated union
│       │   ├── device.ts           ← DeviceSummary, DeviceDetail
│       │   ├── command.ts          ← Command, ParsedIntent, ActionKind
│       │   ├── shadow.ts           ← ShadowResponse, ShadowSummary
│       │   └── telemetry.ts        ← TelemetryReading, TelemetrySource
│       ├── api/
│       │   └── client.ts           ← Typed fetch wrapper (all REST endpoints)
│       ├── stores/
│       │   └── websocket.svelte.ts ← Svelte 5 rune store + auto-reconnect
│       ├── components/
│       │   ├── DeviceCard.svelte   ← Status, last heartbeat, service indicators
│       │   ├── CommandForm.svelte  ← Send + display response (WS push + polling fallback)
│       │   ├── StatusBadge.svelte  ← online/offline/provisioning colored badge
│       │   ├── ShadowPanel.svelte  ← Reported/desired/delta viewer + edit desired
│       │   ├── TelemetryPanel.svelte ← Source tabs + sparkline charts per metric
│       │   ├── SparklineChart.svelte ← Pure SVG line chart, hover tooltips
│       │   ├── JsonView.svelte     ← Recursive JSON renderer, delta key highlighting
│       │   └── ServiceIndicator.svelte ← Status dot + label (Ollama, CAN, MQTT)
│       └── utils/
│           ├── format.ts           ← timeAgo, shortDateTime, formatUptime
│           └── device.ts           ← formatHardwareType (handles {custom:string})
└── vite.config.ts                  ← /api proxy → API_URL, ws:true for WebSocket
```

### WebSocket Store

```typescript
// Auto-reconnect with exponential backoff (1s, 2s, 4s, 8s, max 30s)
export const wsStore = createWsStore();

// Layout mounts on startup:
wsStore.connect(`${apiBase}/api/v1/ws`);

// Components subscribe to events:
$effect(() => {
  const unsub = wsStore.onEvent((event: WsEvent) => {
    if (event.type === "command_response" && event.command_id === myId) {
      response = event;
    }
  });
  return unsub;
});
```

The layout shows a "Live" green dot when WebSocket is connected, "Connecting" amber when reconnecting.

### CommandForm Response Handling

Three-tier response strategy for robustness:

```
1. WebSocket push (instant)
   └─ CommandResponse WsEvent arrives → render immediately

2. Polling fallback (every 3s)
   └─ GET /api/v1/commands/{id} → check status
   └─ Activates if WS push doesn't arrive within 3s

3. Timeout (60s)
   └─ Show "no response received" error
   └─ Elapsed time counter shown during wait
```

---

## 14. AWS Infrastructure

Provisioned via Terraform in `infra/modules/`:

### networking

- VPC with configurable CIDR
- 2 Availability Zones: 1 public subnet + 1 private subnet per AZ
- Single NAT Gateway (dev cost optimization; HA in production requires one per AZ)
- Internet Gateway for public subnets
- Route tables + security groups for API, DB, Lambda

### iot-core

- Thing type: `fleet-device` (common attributes: firmware_version, hardware_type, location)
- Thing group: `fleet-{fleet_id}` with dynamic membership
- IoT Policy: scoped per-device (`iot:Connect`, `iot:Publish`, `iot:Subscribe`, `iot:Receive` — restricted to `fleet/{fleet_id}/{device_id}/*` topics)
- Topic rules: Log MQTT traffic → CloudWatch Logs

### compute

- Lambda function (Rust, `provided.al2023`, ARM64) — placeholder for command routing
- API Gateway HTTP API → Lambda integration
- IAM role with least-privilege Bedrock + IoT permissions

### data

- RDS PostgreSQL 16 (Multi-AZ disabled in dev; enable in production)
- Secrets Manager: random password for DB credentials
- DB subnet group in private subnets

### monitoring

- CloudWatch Alarms: Lambda errors, Lambda duration P99, API 5xx rate, API latency P99, RDS CPU, RDS connections, RDS storage
- CloudWatch Dashboard: All metrics in one view

---

## 15. Security Model

### Transport Security

| Path | Protocol | Auth |
|------|----------|------|
| Device ↔ AWS IoT Core | MQTT over TLS 1.3 (port 8883) | X.509 mutual TLS per device |
| Browser ↔ Cloud API | HTTPS / WSS | None (PoC phase; JWT planned) |
| Cloud API ↔ AWS Bedrock | HTTPS | IAM role credentials |
| Cloud API ↔ RDS | TLS inside VPC | DB password in Secrets Manager |

### Per-Device X.509 Certificates

Each device has a unique certificate issued by AWS IoT Core CA:
- Private key never leaves the device
- Certificate ID stored in `DeviceInfo.certificate_id`
- IoT policy restricts each cert to only its own MQTT topics
- Certificate rotation: revoke old, issue new (handled via IoT Core console / API)

### CAN Bus Safety

- **Read-only mode**: OBD-II modes 2, 5, 10, 14 (write/actuate) are blocked in the `CanInterface` safety check
- Only modes 1, 3, 4, 6, 9 (read sensor/DTC data) are permitted
- ISO 21434 alignment is a target for production certification
- `MockCanInterface` used in all current environments; `SocketCanInterface` guarded behind Phase 2

### Shell Command Security (Defense-in-Depth)

1. **Cloud inference sanitization**: `sanitize_shell_command()` strips from first metacharacter — applied to both Bedrock and Ollama-generated commands before they reach the agent
2. **Agent-side validation**: 5 independent checks before execution (see Section 8)
3. **No shell interpreter**: `tokio::process::Command` spawns the process directly — no `/bin/sh -c` wrapping
4. **Output cap**: 8 KB — prevents accidental large file exfiltration via `cat`

### Command Integrity

- `CommandEnvelope.id` is UUIDv7 (time-sortable, globally unique)
- `correlation_id` ties each request to exactly one response
- Full command audit trail stored in DB/in-memory and exposed via `/api/v1/commands`
- All commands visible to all operators (no per-user scoping in PoC)

---

## 16. Key Design Patterns

### Trait-Based Hardware Abstraction

Every hardware boundary is a trait with a mock:

```
CanInterface      → MockCanInterface     (CAN hardware simulation)
LogSource         → MockLogSource        (filesystem simulation)
Channel           → MockChannel          (MQTT simulation)
InferenceEngine   → MockEngine           (LLM simulation in e2e tests)
```

This enables 402 Rust tests to run with zero hardware dependencies.

### Dual-Mode AppState

`AppState` functions with or without a PostgreSQL database. When `DATABASE_URL` is absent:
- `devices`, `commands`, `shadows` live in `Arc<RwLock<HashMap<...>>>`
- Sample data auto-populated via `with_sample_data()`
- All tests pass without a running database

When `DATABASE_URL` is present:
- Reads/writes go to PostgreSQL via SQLx runtime queries (not compile-time macros)
- In-memory maps serve as a cache/fallback

### Broadcast WebSocket Events

All state changes emit a `WsEvent` via `tokio::sync::broadcast::Sender<WsEvent>` (capacity 256). The WebSocket handler loops on `broadcast::Receiver::recv()` and forwards each event to connected clients. This decouples REST handlers from WebSocket delivery — any endpoint can broadcast without knowing about WebSocket subscribers.

### MQTT Payload Size Management

The response path has a three-layer size budget:

```
Tool executes → response_data can be large (e.g., 69K syslog lines)
                          │
                          ▼
cap_response_size() (fleet agent, before MQTT publish):
  Strategy 1: Trim entries[] array from oldest end until payload < 128 KB
  Strategy 2: If still too large, set response_data = null, keep response_text
                          │
                          ▼
MAX_MQTT_PAYLOAD = 128 KB (code-level cap, matches AWS IoT Core limit)
                          │
                          ▼
rumqttc max_packet_size = 256 KB (client-side buffer, above broker limit)
```

### Inference Engine Separation

`INFERENCE_ENGINE` env var selects one engine at startup — no cascading:

```
INFERENCE_ENGINE=local   → RuleBasedEngine only   (default, $0, <1 ms)
INFERENCE_ENGINE=bedrock → BedrockEngine only      (cloud LLM, ~$0.001)
```

`TieredEngine` (rule-based → Bedrock cascade) is implemented and tested but removed from
the main startup path to keep inference behavior predictable and cost-controllable.

### UUIDv7 for Time-Sorted IDs

All `CommandEnvelope.id` and `DeviceInfo.id` values are UUIDv7. This makes them
lexicographically sortable by creation time without a separate `created_at` index scan,
which is important for the commands audit trail query.

---

## 17. Performance Targets

| Metric | Target | Current status |
|--------|--------|----------------|
| Local inference latency (rule-based) | <1 ms | ✅ Pattern match in μs |
| Local inference latency (Ollama phi3:mini) | <200 ms p95 | ✅ ~50–200 ms on x86 |
| Bedrock inference latency | <2000 ms p95 | ✅ ~200–1500 ms (cold starts up to 1500 ms) |
| Local query coverage | ≥80% | ✅ Rule engine matches ~80% |
| MQTT round-trip (command → response) | <500 ms for log/shell | ✅ Typical: 100–300 ms |
| Fleet agent binary size | <10 MB | ✅ ~8.8 MB |
| Fleet agent cold start | <10 ms | ✅ ~10 ms (Rust, no GC) |
| Fleet agent RAM | <10 MB | ✅ ~5 MB (Rust, no runtime overhead) |
| Per-device monthly cost (50 devices) | <$5 | ✅ At 80% local coverage |
| Unauthorized command prevention | 100% blocked | ✅ Allowlist enforced |
| Log analysis accuracy | >95% error detection | ✅ 9 pattern categories |
