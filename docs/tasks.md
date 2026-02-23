# Task Tracker

## Phase 1: Core Crates (zc-protocol, zc-canbus-tools, zc-log-tools)
- [x] Shared types: commands, telemetry, device, DTC, shadows, topics
- [x] CAN bus / OBD-II diagnostic tools (5 tools, trait-based)
- [x] Multi-format log parsing + 4 analysis tools

## Phase 2: MQTT Channel + Fleet Agent
- [x] MQTT channel abstraction for AWS IoT Core (mTLS)
- [x] Edge agent binary (tool registry, command executor, MQTT event loop)
- [x] Heartbeat publishing with uptime tracking

## Phase 3: Cloud API + MQTT Bridge
- [x] Axum REST API (devices, commands, telemetry, heartbeat)
- [x] Device provisioning (POST /api/v1/devices)
- [x] Telemetry ingestion endpoint
- [x] MQTT bridge (cloud subscribes to fleet topics, reuses handler logic)
- [x] WebSocket real-time push (broadcast events)
- [x] Inference engine (RuleBasedEngine + BedrockEngine + TieredEngine)

## Phase 4: Ollama Local Inference
- [x] OllamaClient HTTP client for /api/chat
- [x] System prompt with 9-tool schema
- [x] Integration into CommandExecutor (local inference before tool dispatch)
- [x] AgentConfig TOML loading with OllamaConfig

## Phase 5: End-to-End Integration Tests
- [x] TestHarness in zc-e2e-tests (35 tests)
- [x] Cross-crate integration coverage
- [x] 324 Rust tests passing, clippy clean, fmt clean

## Phase 6: Device Shadow Sync
- [x] Migration 005_device_shadows.sql
- [x] DB queries: get, list, upsert_reported (JSONB merge), set_desired
- [x] 3 REST endpoints (list, get with computed delta, set desired)
- [x] Fleet agent shadow_sync task (periodic reporter)
- [x] Shadow delta handler with ack on mqtt_loop
- [x] compute_delta logic in mqtt_bridge
- [x] 5 E2E shadow sync tests
- [x] Frontend types + API client methods for shadows
- [x] 324 total tests, clippy clean, svelte-check clean

## Phase 7: Local Full-Loop Smoke Test
Test the complete command lifecycle on a single x86 machine: frontend → cloud API → MQTT → fleet agent → tool execution → response back to frontend.

- [x] Local MQTT broker setup (mosquitto, no TLS)
- [x] Fleet agent config for local dev (mock CAN, real logs, Ollama, plaintext MQTT)
- [x] Cloud API config for local dev (in-memory state, plaintext MQTT, PORT env var)
- [x] Verify Ollama model available (phi3:mini)
- [x] Start all services locally (mosquitto :1883, cloud API :3002, fleet agent)
- [x] Test loop: send command via curl → cloud API → MQTT → fleet agent → response back (search_logs succeeded, read_dtcs mock timeout as expected)
- [x] Test log tool commands (search_logs on /var/log/syslog — 69K lines scanned, 0 matches, success)
- [x] Test CAN tool commands (read_dtcs — mock timeout expected, response propagated correctly)
- [x] Test heartbeat + shadow sync visible via API (heartbeats updating last_heartbeat, shadow v5 with uptime/tool/command state)
- [x] Frontend proxy wired (vite.config.ts API_URL env var, ws:true for WebSocket)
- [x] Document local dev setup (docs/test.md — manual test guide)

## Phase 7b: Frontend Display Bugs
Found during full-loop UI testing. Not committed yet — fix and commit together.

- [x] Hardware field shows `[object Object]` for custom types — fixed: `formatHardwareType()` helper in device.ts, used in DeviceCard + device detail page
- [x] Fleet ID shows UUID instead of `"local-fleet"` on device detail page — fixed: displays `metadata.fleet` with fallback to `fleet_id`
- [x] CommandForm fleet_id fix already applied (reads `metadata.fleet` first)
- [x] TypeScript HardwareType updated to match Rust serde representation (string | {custom: string})

## Phase 8: Agent Mode — From Tool Router to AI Agent
Transform the fleet agent from a rigid tool router into a true AI agent with three action types.

- [x] Add ActionKind enum (Tool/Shell/Reply) to zc-protocol ParsedIntent (backward-compatible via #[serde(default)])
- [x] Create shell.rs module — safe shell executor with allowlist, injection blocking, path blocking, 5s timeout, 64KB cap
- [x] Add shell-words + thiserror crate deps to zc-fleet-agent
- [x] Rewrite Ollama system prompt for three action types (tool/shell/reply)
- [x] Update RawIntent to parse action, command, message fields
- [x] Update CommandExecutor with action routing: Tool → ToolRegistry, Shell → shell::execute(), Reply → return message
- [x] Set explicit ActionKind::Tool in cloud-side rules.rs (all ParsedIntent constructions)
- [x] Update bedrock.rs system prompt + LlmResponse for three action types
- [x] Update tiered.rs test MockEngine with ActionKind::Tool
- [x] Fix E2E tests (error_paths.rs, inference_paths.rs) — add ActionKind::Tool to ParsedIntent constructions
- [x] Frontend: add ActionKind type, update ParsedIntent interface
- [x] Frontend: update CommandForm — action label/color, monospace pre for shell output, neutral "sent to device" message
- [x] 367 Rust tests passing (up from 324), clippy clean, fmt clean, svelte-check clean

## Later
- [ ] Real CAN bus interface (SocketCanInterface send/recv)
- [ ] REST API auth middleware (JWT or API keys)
- [ ] Deployment pipeline (Lambda handler, CI/CD)
- [ ] Bedrock cloud inference end-to-end wiring
- [ ] Frontend: shadow UI page, telemetry charts, richer device detail
