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

## Phase 9: Frontend Polish — Shadow UI, Telemetry Charts, Richer Device Detail
Frontend-only phase. No backend changes. All data already available via existing APIs and WS events.

- [x] Add TelemetrySource type + TelemetryReading interface to types/index.ts
- [x] Create shared format utils (timeAgo, shortDateTime, formatUptime) in utils/format.ts
- [x] Update DeviceCard to use shared timeAgo from utils/format
- [x] Create JsonView component — recursive JSON renderer with delta key highlighting
- [x] Create ShadowPanel component — shadow list, detail (reported/desired/delta), edit desired, WS auto-refresh
- [x] Create SparklineChart component — pure SVG line chart, hover tooltips, zero dependencies
- [x] Create TelemetryPanel component — source filter tabs, sparkline charts per metric, text/json table
- [x] Create ServiceIndicator component — status dot + label card for service status
- [x] Rework device detail page with 4-tab layout (Overview, Commands, Shadows, Telemetry)
- [x] Overview tab: device info cards, service status row (from "state" shadow), heartbeat pulse
- [x] Commands tab: CommandForm + device-specific command history with WS live updates
- [x] Shadows tab: ShadowPanel component
- [x] Telemetry tab: TelemetryPanel component
- [x] WS subscriptions: heartbeat, status_changed, command events, shadow updates
- [x] pnpm check passes (0 errors, 0 warnings)
- [x] pnpm build succeeds (production build)

### Bug fix: MQTT response payload truncation
- [x] Cap serialized CommandResponse to 9KB before MQTT publish (tool response_data was unbounded)
- [x] Truncation preserves tool summary in response_text, marks response_data as truncated
- [x] 3 new tests (small passthrough, oversized truncated, no-data unaffected)
- [x] 379 total Rust tests passing, clippy clean

## Phase 10: query_journal Tool — Systemd Journal Queries
- [x] New tool: crates/zc-log-tools/src/tools/query_journal.rs (QueryJournal implementing LogTool)
- [x] Update tools/mod.rs: 4 → 5 tools, add QueryJournal to all_tools()
- [x] Update fleet-agent inference.rs: SYSTEM_PROMPT + KNOWN_TOOLS (9 → 10)
- [x] Update fleet-agent registry.rs: test assertions (9 → 10)
- [x] Update cloud-api rules.rs: pattern matching for journal/service queries + extract_service_name helper
- [x] Update cloud-api bedrock.rs: SYSTEM_PROMPT + KNOWN_TOOLS (9 → 10)
- [x] Update e2e inference_paths.rs: rename + add query_journal to tool_commands
- [x] Update e2e shadow_sync.rs: tool_count fixture 9 → 10
- [x] 392 tests passing (up from 379), clippy clean, fmt clean

## Phase 11: Response Data Pipeline + MQTT Packet Fix + Robust Frontend
Fix response_data being lost in transit from edge agent → cloud API → WebSocket → frontend.

- [x] Add `response_data` field to `WsEvent::CommandResponse` (events.rs)
- [x] Forward `response_data` in both broadcast sites (mqtt_bridge.rs, routes/responses.rs)
- [x] Add `response_data` to frontend WsEvent type (types/index.ts)
- [x] Raise `MAX_MQTT_PAYLOAD` from 9KB to 128KB (AWS IoT Core limit)
- [x] Smart entry trimming in `cap_response_size` — trims oldest log entries to fit before nuclear drop
- [x] Raise rumqttc `max_packet_size` from 10KB default to 256KB (both TLS and plaintext constructors)
- [x] Rewrite CommandForm with robust response handling: WS push + polling fallback (3s) + timeout (60s)
- [x] Add elapsed time counter to "Waiting for response" indicator
- [x] Render log entries as readable lines (not raw JSON) when response_data has entries array
- [x] Update README: log tools 4→5, agent mode, shadow/telemetry endpoints, local dev setup, test count
- [x] 393 tests passing, clippy clean, fmt clean, svelte-check clean

## Phase 12: Bedrock Cloud Inference End-to-End Wiring
Fix critical bug where Bedrock engine was silently dropped in no-DB path, improve defaults, add docs.

- [x] Fix inference engine dropped in no-DB path — add `with_sample_data_and_inference()` to AppState
- [x] Use new constructor in main.rs else branch so TieredEngine is preserved
- [x] Add startup logging for AWS region and active inference tier
- [x] Increase default Bedrock timeout 5s → 15s (cold starts can take 8-10s)
- [x] Fix `validate_reply` confidence — `call.confidence.max(1.0)` was a no-op, changed to `1.0`
- [x] Add Bedrock testing section to docs/test.md

## Phase 13: Shell Command Fixes + Inference Engine Separation
Fix shell commands failing due to pipe injection and missing binaries, show real errors in frontend, and separate inference engines into clean either/or config.

- [x] Add shell command patterns to cloud-side RuleBasedEngine (ip addr, cpu temp, gpu temp, disk, memory, uptime, kernel, cpu info, processes, hostname)
- [x] Add no-pipes rule + shell examples to Bedrock system prompt
- [x] Add defense-in-depth shell sanitization in edge executor (strips metacharacters from cloud intents)
- [x] Make `sanitize_shell_command` public in inference.rs for executor reuse
- [x] Add `error` field to `WsEvent::CommandResponse` (events.rs, mqtt_bridge.rs, responses.rs)
- [x] Add `error` field to frontend WsEvent type (types/index.ts)
- [x] Update CommandForm to show actual device error message instead of generic text
- [x] Replace `BEDROCK_ENABLED` boolean with `INFERENCE_ENGINE=local|bedrock` env var (either/or, no cascade)
- [x] Remove TieredEngine from main.rs pipeline (module kept for future use)
- [x] 9 new shell command tests in RuleBasedEngine
- [x] 402 tests passing (up from 393), clippy clean, fmt clean, svelte-check clean
- [x] E2E verified: ip address, cpu temp, disk space, memory, kernel version, system logs all working

## Phase 15: Expand Rule-Based Engine + Wire Tiered Inference
Fix: "which application is consuming CPU?" failed because no rule-based pattern existed, falling through to Bedrock which generated `top -o %CPU -n 1` (missing `-b` batch flag).

- [x] Add 10 new rule patterns to rules.rs (top, sensors, dmesg, ss, du, lsblk, date, whoami, systemctl, ethtool)
- [x] Wire `INFERENCE_ENGINE=tiered` match arm in main.rs (local rules first, Bedrock fallback)
- [x] Update Bedrock system prompt with 10 new shell examples + batch mode notes
- [x] Update Ollama system prompt with 10 new shell examples + batch mode notes
- [x] Change run-local.sh from `bedrock` to `tiered`
- [x] Fix `unrecognized_returns_none` test (now matches `date` rule)
- [x] Add 21 new tests for new patterns + regression test
- [x] 452 tests passing (up from 402), clippy clean, fmt clean

## Phase 16: Hella BCR UDS Integration
- [x] ECU profile definitions (ecu_profile.rs) — BCR (0x60D/0x58D) + BCF (0x609/0x589) with known DIDs
- [x] UDS service allowlist (uds_safety.rs) — read-only: 0x10, 0x19, 0x22, 0x3E; blocks writes/flash/security
- [x] UDS protocol helpers (uds.rs) — request builders, send/receive, ISO-TP multi-frame, negative response handling, NRC descriptions
- [x] New tool: read_uds_dtcs — Read DTCs via UDS 0x19 (ReadDTCInformation) for BCR/BCF
- [x] New tool: read_uds_did — Read DIDs via UDS 0x22 (ReadDataByIdentifier) — single or all known DIDs
- [x] New tool: uds_session_control — Session control (0x10) + TesterPresent (0x3E), programming session blocked
- [x] UDS error variants (error.rs) — UdsSafetyViolation, UdsNegativeResponse, UnknownEcu
- [x] UDS safety checks in MockCanInterface and SocketCanInterface — validates service ID for ECU request CAN IDs
- [x] Updated all_tools() — 5 → 8 CAN bus tools
- [x] Updated fleet agent registry (13 tools: 8 CAN + 5 log)
- [x] Updated fleet agent inference.rs SYSTEM_PROMPT + KNOWN_TOOLS (10 → 13)
- [x] Updated cloud-side rules.rs with BCR/BCF/Hella UDS patterns (9 new test cases)
- [x] Updated cloud-side bedrock.rs system prompt + KNOWN_TOOLS (10 → 13)
- [x] Updated E2E tests (tool count assertions, inference path tests)
- [x] 535 tests passing (up from 452), clippy clean, fmt clean

## Phase 17: DTC Description Database
Embed Wal33D/dtc-database (18,805 codes, MIT) + UDS Failure Type Byte decoder.
See docs/future-implementation.md for full research.

- [x] Data prep script: `scripts/prepare_dtc_data.py` — downloads Wal33D SQLite DB, extracts TSV (pinned commit SHA, metadata headers)
- [x] TSV data: 9,415 generic + 9,390 manufacturer codes in `crates/zc-canbus-tools/data/`
- [x] Rewrite `dtc_db.rs` — `include_str!` + `LazyLock<HashMap>`, generic + manufacturer lookup, severity heuristic
- [x] Create `ftb.rs` — Failure Type Byte decoder (~40 entries per ISO 14229-1 Annex D)
- [x] Add `failure_type`, `raw_dtc`, `severity_source` fields to `DtcCode` in zc-protocol (backward-compatible)
- [x] Update `read_uds_dtcs` — decode FTB, look up descriptions, preserve raw 3-byte DTC, 5-char code format
- [x] Verify `read_dtcs` — works with new database, populates severity_source
- [x] Wire `pub mod ftb` into `lib.rs`
- [x] Add frontend TypeScript types: `DtcCode`, `DtcSeverity`, `DtcCategory` interfaces
- [x] Add DTC-aware rendering in CommandForm: severity-colored cards, description, failure type, raw hex, category labels
- [x] Tests: dtc_db (generic/mfr lookup, severity inference, data integrity, duplicate detection), ftb (known/reserved/format), protocol (roundtrip, omission, backward compat), UDS tool (FTB decode, 5-char code, heuristic severity)
- [x] Update docs: CLAUDE.md (status, tool counts), architecture.md (DtcCode struct, tool table, DTC DB description), future-implementation.md (Phase 17 done, gap table updated)
- [x] 564 tests passing, clippy clean, fmt clean, svelte-check clean

## Phase 18: ECU Wakeup + UDS Robustness
BCR requires vehicle speed wakeup (CAN 0x98) before responding to UDS. Generic ECUs unaffected.

- [x] Add optional `WakeupConfig` to `EcuProfile` (CAN ID, frame data generator, interval, repeat count)
- [x] Implement vehicle speed wakeup for Hella BCR (CRC-protected 0x98 frames, 80ms interval, 16 CRC × 2 cycles)
- [x] Execute wakeup automatically in `uds_query`/`uds_query_isotp` when profile has wakeup config
- [x] Add NRC 0x78 ("response pending") retry logic in UDS query layer (generic, all ECUs)
- [x] Tests: wakeup frame generation, wakeup execution before UDS, NRC 0x78 retry, profiles without wakeup unaffected
- [ ] Deploy to S32G and verify BCR DTC reads succeed end-to-end via frontend

## Phase 19: Agent Recovery Loop — AI Retry on Tool Failure
When a tool fails or a shell command errors, the executor should attempt intelligent
recovery instead of returning the raw error. Tiered: rule-based recovery first (free,
<1ms), then Ollama (local LLM, if available), then Bedrock via MQTT (cloud LLM fallback).

### Recovery triggers
- **Tool execution errors** (source not found, timeout, unknown ECU, etc.) → always retry
- **Shell errors** (command not found, empty stdout) → always retry
- **Tool success with empty data** → do NOT retry (empty is a valid answer)

### Architecture
- Recovery loop lives in executor.rs (edge-side), wrapping execute_single()
- Max 2 attempts total (1 original + 1 retry), shared timeout budget (15s)
- Recovery context includes: original query, tool attempted, args, error, available tools
- Frontend shows attempt chain ("Attempt 1: failed → Attempt 2: success")
- Full attempt history in CommandResponse metadata for audit trail

### Tasks
- [ ] Define `RecoveryContext` struct (original query, failed tool, error, available tools, device capabilities)
- [ ] Add rule-based recovery rules in executor (e.g., syslog not found → query_journal, OBD timeout → try UDS equivalent)
- [ ] Add `recover()` method to executor: rules → Ollama → Bedrock (tiered, like inference)
- [ ] Add Ollama recovery prompt (tool failed with error X, original query Y, suggest alternative)
- [ ] Add Bedrock recovery via MQTT request/response (new topic: `fleet/{id}/{id}/recovery/request|response`)
- [ ] Wrap execute() in agent loop: attempt → check result → recover → retry (max 2 attempts)
- [ ] Add `attempts` field to CommandResponse (Vec of attempt summaries with tool, args, result, duration)
- [ ] Frontend: render attempt chain in CommandForm (show retry history, not just final result)
- [ ] Add device capability profile to heartbeat/shadow (has_journald, has_syslog, can_interfaces, etc.)
- [ ] Send device capabilities to cloud so first-attempt inference is more accurate
- [ ] Tests: recovery on tool error, recovery on shell error, no recovery on success, no recovery on empty-but-valid, max retry limit, timeout budget respected
- [ ] Cost/observability: log recovery tier used, track retry rate per tool for rule engine improvement

## Phase 20: Clear DTCs Tool
- [ ] Safety review: require confirmation parameter (`"confirm": true`)
- [ ] OBD-II Mode 0x04 — `clear_dtcs` tool
- [ ] UDS Service 0x14 — `clear_uds_dtcs` tool (selective clearing by group)
- [ ] Add 0x04/0x14 to safety allowlist (with confirmation gate)
- [ ] Rule engine + Bedrock patterns for clear commands

## Phase 21: Expanded PID Coverage (8 → 200+)
- [ ] Create `pid_database.rs` — static PID catalog with formulas and units
- [ ] PID formula engine (runtime evaluation of `(256*A + B) / 4` expressions)
- [ ] Update `read_pid` to accept any supported PID
- [ ] Unit conversion (metric/imperial)

## Phase 22: VIN Decoder (NHTSA VPIC, public domain)
- [ ] Offline VPIC SQLite database
- [ ] WMI lookup, SAE J287 checksum, pattern matching
- [ ] Update `read_vin` tool with decoded make/model/year/engine

## Later
- [x] Wire SocketCanInterface to real socketcan (conditional on Linux + config.can_interface, graceful fallback to mock)
- [ ] Advanced DTC features: pending (0x07), permanent (0x0A), status byte, I/M readiness, DTC snapshots
- [ ] Fleet-wide DTC aggregation, trend analysis, AI interpretation
- [ ] DBC file parser for CAN signal-level decode
- [ ] REST API auth middleware (JWT or API keys)
- [ ] Deployment pipeline (Lambda handler, CI/CD)
