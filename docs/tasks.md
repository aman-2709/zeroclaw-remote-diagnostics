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

## Remaining Work
- [ ] Real CAN bus interface (SocketCanInterface send/recv)
- [ ] REST API auth middleware (JWT or API keys)
- [ ] Deployment pipeline (Lambda handler, CI/CD)
- [ ] Bedrock cloud inference end-to-end wiring
- [ ] Frontend: shadow UI page, telemetry charts, richer device detail
