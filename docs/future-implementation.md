# Future Implementation — Diagnostic Feature Parity & Beyond

Research conducted 2026-03-11 across 10+ open-source automotive diagnostic projects
to understand how professional OBD-II scanners work and plan feature parity.

## How OBD-II / UDS Diagnostics Work

### Two Protocol Layers

| | Generic OBD-II (SAE J1979) | Enhanced UDS (ISO 14229) |
|---|---|---|
| Scope | Emission-related only | All vehicle systems |
| CAN IDs | Fixed: `0x7DF` broadcast, `0x7E8-0x7EF` response | OEM-specific per ECU (e.g., Hella BCR: `0x60D`/`0x58D`) |
| DTC format | 2 bytes → 5-char code (P0420) | 3 bytes → 5-char + failure type byte |
| Services | 10 modes (0x01-0x0A) | ~26 services (0x10-0x87) |
| Any scanner | Yes, $20 ELM327 | Requires ECU addresses, DIDs, security algorithms |

### DTC Encoding (ISO 15031-6)

2-byte wire format decodes to 5-character alphanumeric code:

```
Byte 1: [A7 A6][A5 A4][A3 A2 A1 A0]
         ^^^^^^ ^^^^^^ ^^^^^^^^^^^^^
         Category Origin  Digit 2
         00=P    00=std(0)
         01=C    01=mfr(1)
         10=B    10=std(2)
         11=U    11=joint(3)

Byte 2: [B7 B6 B5 B4][B3 B2 B1 B0]
         Digit 3       Digit 4

UDS adds Byte 3: Failure Type Byte (FTB)
  0x11=short to ground, 0x12=short to battery,
  0x13=open circuit, 0x16=voltage below threshold, etc.
```

Example: `0x04 0x20` → P0420 (Catalyst Efficiency Below Threshold)

### DTC Description Sources (Layered)

1. **SAE J2012** (~11,000 standardized codes): P0xxx, P2xxx, C0xxx, B0xxx, U0xxx — universal
2. **Manufacturer-specific** (P1xxx, B1xxx, C1xxx, U1xxx): Proprietary per OEM
3. **ODX/PDX files** (ISO 22901): XML diagnostic databases authored per ECU by OEMs
4. **Professional tools** get descriptions via: OEM licensing, ODX access, reverse engineering

### Key OBD-II Services

| Mode | Purpose | Our Tool |
|---|---|---|
| 0x01 | Live sensor PIDs | `read_pid` |
| 0x02 | Freeze frame data | `read_freeze` |
| 0x03 | Stored emission DTCs | `read_dtcs` |
| 0x04 | Clear DTCs | Not yet |
| 0x07 | Pending DTCs | Not yet |
| 0x09 | Vehicle info (VIN) | `read_vin` |
| 0x0A | Permanent DTCs | Not yet |

### Key UDS Services (ISO 14229)

| SID | Service | Our Tool | Status |
|---|---|---|---|
| 0x10 | DiagnosticSessionControl | `uds_session_control` | Done |
| 0x14 | ClearDiagnosticInformation | Not yet | Planned |
| 0x19 | ReadDTCInformation | `read_uds_dtcs` | Done |
| 0x22 | ReadDataByIdentifier | `read_uds_did` | Done |
| 0x27 | SecurityAccess | Not yet | Future (security review needed) |
| 0x2E | WriteDataByIdentifier | Not yet | Future (safety review needed) |
| 0x2F | IOControl (actuator tests) | Not yet | Future (safety review needed) |
| 0x31 | RoutineControl | Not yet | Future |
| 0x3E | TesterPresent | `uds_session_control` | Done |

---

## Feature Gap Analysis vs $500 Scanners

| Capability | $500 Scanner | Our Project | Gap |
|---|---|---|---|
| Read generic DTCs | Mode 0x03 + descriptions | `read_dtcs` + 18K-code database | Done (Phase 17) |
| Read UDS DTCs | UDS 0x19 + ODX/PDX | `read_uds_dtcs` + descriptions + FTB | Done (Phase 17) |
| Clear DTCs | Mode 0x04 / UDS 0x14 | Not implemented | **New tool** |
| Live sensor data | 200+ PIDs | `read_pid` (8 PIDs) | **Expand PID coverage** |
| Freeze frame | Mode 0x02 | `read_freeze` (exists) | Minor gaps |
| VIN decode | Mode 0x09 + NHTSA VPIC | `read_vin` (raw only) | **VIN decoder** |
| DTC descriptions | Proprietary DB (10K+) | 18,805 codes (Wal33D, MIT) | Done (Phase 17) |
| Failure type decode | ISO 15031-6 FTB | `ftb.rs` (~40 entries) | Done (Phase 17) |
| Manufacturer DTCs | Licensed ODX/PDX | 9,390 mfr-specific codes | Done (Phase 17) |
| I/M readiness | Mode 0x01 PID 0x01 | Not implemented | Future |
| Pending DTCs | Mode 0x07 | Not implemented | Future |
| Permanent DTCs | Mode 0x0A | Not implemented | Future |
| Actuator tests | UDS 0x2F IOControl | Not implemented | Future |
| **Remote access** | None (handheld) | **Full MQTT + web UI** | We lead |
| **Fleet diagnostics** | None | **Multi-device dashboard** | We lead |
| **AI interpretation** | None | **NL commands + LLM** | We lead |

---

## DTC Database Options (Researched)

### Best Option: Wal33D/dtc-database (MIT)

- **URL**: https://github.com/Wal33D/dtc-database
- **License**: MIT — fully compatible
- **Total**: 18,805 rows (12,128 unique codes)
- **Generic SAE**: 9,415 codes
- **Manufacturer-specific**: 9,390 across 33 manufacturers
- **Breakdown**: P: 14,821 | B: 1,465 | C: 985 | U: 1,534
- **Format**: SQLite + plain text source files (`CODE - Description`)
- **Quality**: Excellent — only 2 vague entries out of 18,805
- **Includes**: Extended hex codes (P000A through P34C8)

### Other Options Evaluated

| Source | Codes | License | Notes |
|---|---|---|---|
| xinings/DTC-Database | 6,665 | **No license** | Cannot legally embed |
| fabiovila/OBDIICodes | 2,381 | MIT | P-codes only, no B/C/U |
| mytrile/obd-trouble-codes | 3,071 | MIT | Broken JSON, missing P2xxx/U0xxx |
| pyobd (barracuda-fsh) | 2,193 | GPL-2.0 | P-codes only, GPL incompatible |
| AndrOBD (fr3ts0n) | 1,356 generic + 650 VAG | GPL-3.0 | GPL incompatible, but has 38 language translations |
| OBDium (provrb) | SQLite DB | Non-commercial only | Cannot embed commercially |
| todrobbins/dtcdb | ~350 | MIT | Too small |
| dsoprea/DtcLookup | 21 mfr files | GPL-2.0 | GPL incompatible |

### UDS Failure Type Byte (FTB) — ISO 15031-6

~40 standard values. No open-source database found. Must define manually from publicly
available technical references. Key values:

| FTB | Description |
|---|---|
| 0x01 | General Electrical Failure |
| 0x02 | General Signal Failure |
| 0x11 | Circuit Short to Ground |
| 0x12 | Circuit Short to Battery |
| 0x13 | Circuit Open |
| 0x14 | Circuit Short to Ground or Open |
| 0x15 | Circuit Short to Battery or Open |
| 0x16 | Circuit Voltage Below Threshold |
| 0x17 | Circuit Voltage Above Threshold |
| 0x1F | Circuit Intermittent |
| 0x41 | General Checksum Failure |
| 0x42 | General Signal Plausibility Failure |
| 0x44 | Signal Too Low |
| 0x45 | Signal Too High |
| 0x46 | Signal Out of Allowed Range |
| 0x47 | Signal Stuck Low |
| 0x48 | Signal Stuck High |
| 0x49 | Signal Conditions Not Correct |
| 0x4A | Signal Erratic |
| 0x51 | Not Activated |
| 0x52 | Not Deactivated |
| 0x54 | Emergency Deactivation |
| 0x55 | Set Value Not Reached |
| 0x62 | Timeout / Not Available |
| 0x64 | Signal Plausibility Failure |
| 0x86 | Component Obstructed / Blocked |
| 0x87 | Component Over Temperature |
| 0x93 | No Communication |
| 0x96 | Component Internal Failure |
| 0x97 | Component Locked |
| 0xFF | No Failure Type Information |

### UDS NRC (Negative Response Codes) — ISO 14229

Available in `automotive_diag` Rust crate (MIT/Apache-2.0). 40 codes (0x10-0x93).
We already have NRC decoding in our `uds.rs`.

---

## Open-Source Projects Reviewed

### OBDium (Rust) — https://github.com/provrb/obdium
- Rust OBD-II tool with Tauri desktop GUI
- 200+ PIDs, DTC read/display, VIN decode via NHTSA VPIC (offline SQLite)
- Session recording/replay (JSON) — useful testing pattern
- DTC bit-manipulation follows standard ISO 15031-6
- Custom non-commercial license — code/data not reusable
- Uses `evalexpr` crate for runtime PID formula evaluation

### pyobd (Python) — https://github.com/barracuda-fsh/pyobd
- GPL-2.0, Python, ~214 OBD commands, 2,193 P-code descriptions
- Clean `OBDCommand` data model (name, desc, bytes, decoder function)
- UAS (Unit and Scaling) table with `pint` dimensional analysis
- ISO-TP multi-frame assembly with sequence validation

### AndrOBD (Android) — https://github.com/fr3ts0n/AndrOBD
- GPL-3.0, Java, 500+ PIDs, 1,356 generic + 650 VAG-specific DTCs
- **CSV-driven data model** — PIDs, conversions, codes all as external files (no recompile)
- Bit-level PID extraction (offset, length, mask) — multiple values per response byte
- 5 conversion types (Linear, Hash, Bitmap, CodeList, ASCII)
- 38 language translations for DTC descriptions
- VAG KW1281 + Ford CAN manufacturer-specific protocols

### OVMS (MIT) — https://github.com/openvehicles/Open-Vehicle-Monitoring-System-3
- **Most architecturally relevant** — remote vehicle monitoring via MQTT, 40+ vehicles
- MIT license, ESP32 hardware, 300+ standardized metrics
- Remote commands: charge control, climate, door lock, wakeup
- **Poll list pattern**: `(ecu_addr, service, did, {off/awake/charging/on intervals}, bus)` — directly maps to our ECU profiles
- **State-dependent polling** — reduce CAN traffic based on vehicle state
- DBC file parsing for CAN signal-level decode
- Vehicle module factory pattern (similar to our `ecu_profile` system)
- OTA firmware updates with dual-partition A/B rollback
- Duktape JS scripting engine for user automation
- MQTT topic hierarchy: `ovms/{user}/{vehicle}/metric/{name}`, `command/{id}`, `response/{id}`
- Plugin system with community contributions

### FreeDiag — https://github.com/fenugrec/freediag
- GPL-3.0, C, legacy project
- No DTC database, incomplete CAN support
- Limited value — protocol layering patterns (L0/L1/L2/L3) worth noting

### Hella ecuflasher — /home/xl4/dev/xl4-projects/gitlab/hella/ecuflasher-hella
- Internal project — ECU security unlock tool for Hella BCR/BCF
- Confirms BCR CAN IDs (0x60D/0x58D), BCF (0x609/0x589), 250kbps
- LCG + XOR security key algorithm (constant: 0x45766544)
- UDS services: 0x10, 0x11, 0x14, 0x19, 0x22, 0x27, 0x2E, 0x31, 0x34-0x37, 0x3E, 0x85
- Fingerprint DID: 0xF15A, Diagnostic DID: 0xF100
- No DTC database — protocol-level only

---

## Implementation Roadmap

### Phase 17: DTC Description Database (Tier 1) — DONE

Embedded Wal33D/dtc-database (18,805 codes, MIT) into `zc-canbus-tools` as `include_str!` + `LazyLock<HashMap>`.
Added FTB decoder for UDS 3-byte DTCs. New `DtcCode` fields: `failure_type`, `raw_dtc`, `severity_source`.

- [x] Data prep script (`scripts/prepare_dtc_data.py`) — downloads SQLite DB, extracts TSV (pinned commit SHA)
- [x] TSV data files: `crates/zc-canbus-tools/data/dtc_generic.tsv` (9,415) + `dtc_manufacturer.tsv` (9,390)
- [x] Rewrote `dtc_db.rs` — generic + manufacturer lookup, conservative severity heuristic
- [x] Created `ftb.rs` — ~40 FTB entries per ISO 14229-1 Annex D
- [x] Added `failure_type`, `raw_dtc`, `severity_source` to `DtcCode` (backward-compatible)
- [x] `read_dtcs` populates descriptions + severity from database
- [x] `read_uds_dtcs` decodes FTB, looks up descriptions, preserves raw 3-byte hex, 5-char code
- [x] Frontend TypeScript types: `DtcCode`, `DtcSeverity`, `DtcCategory`
- [x] 564 tests passing, clippy clean, fmt clean, svelte-check clean

### Phase 18: Clear DTCs Tool

Add `clear_dtcs` (OBD-II Mode 0x04) and `clear_uds_dtcs` (UDS 0x14) with safety controls.

- [ ] Safety review: require confirmation parameter (`"confirm": true`)
- [ ] OBD-II Mode 0x04 implementation (clears all emission DTCs)
- [ ] UDS Service 0x14 implementation (selective clearing by group)
- [ ] Add to safety allowlist (currently read-only)
- [ ] Rule engine patterns for "clear BCR DTCs", "reset fault codes"
- [ ] Tests with mock interface

### Phase 19: Expanded PID Coverage

Go from 8 PIDs to 200+ with proper unit conversion.

- [ ] Create `pid_database.rs` — static PID catalog (pid, name, formula, unit, min, max)
- [ ] Use AndrOBD's CSV-driven pattern: PID definitions as data, not code
- [ ] Support all standard Mode 0x01 PIDs (SAE J1979)
- [ ] PID formula engine (evaluate `(256*A + B) / 4` style expressions)
- [ ] Update `read_pid` to accept any supported PID (not just 8 hardcoded)
- [ ] Unit conversion (metric/imperial)

### Phase 20: VIN Decoder

Offline VIN decoding using NHTSA VPIC database (public domain).

- [ ] Download VPIC SQLite database from NHTSA
- [ ] WMI (World Manufacturer Identifier) lookup — first 3 chars → manufacturer
- [ ] SAE J287 checksum validation
- [ ] Pattern matching against VPIC tables
- [ ] Extract: make, model, year, engine type, fuel type, drive type, body class
- [ ] Update `read_vin` tool to return decoded info alongside raw VIN

### Phase 21: Advanced DTC Features

- [ ] Pending DTCs (Mode 0x07)
- [ ] Permanent DTCs (Mode 0x0A)
- [ ] DTC status byte decoding (8-bit: testFailed, confirmed, pending, MIL requested, etc.)
- [ ] I/M readiness monitor reading (Mode 0x01 PID 0x01)
- [ ] DTC snapshot/freeze frame per DTC (UDS 0x19 subfunction 0x04)

### Phase 22: Fleet-Wide Diagnostic Intelligence

Features no $500 scanner can do — our differentiators.

- [ ] Fleet-wide DTC aggregation (which codes appear across multiple vehicles?)
- [ ] DTC trend analysis (new since last scan, resolved, recurring)
- [ ] AI-powered DTC interpretation ("explain these 219 codes in plain English")
- [ ] Per-ECU DTC profile files (Hella B1xxx mapping as discovered)
- [ ] Automated diagnostic reports (per-vehicle health summary)

### Phase 23: CAN Signal Decode (DBC Support)

Parse Vector DBC files for signal-level CAN frame decode — understand individual signals
within raw CAN traffic (e.g., "byte 3 bits 0-7 = wheel speed FL, factor 0.01, offset 0").

- [ ] DBC file parser (OVMS has yacc/lex reference, or use `canparse`/`dbc` Rust crate)
- [ ] Signal extraction from raw CAN frames
- [ ] Integration with `can_monitor` tool (decoded signal view)
- [ ] Per-vehicle DBC file configuration

### Useful Rust Crates to Evaluate

| Crate | Purpose | License |
|---|---|---|
| `automotive_diag` | UDS/OBD-II/KWP2000 enums, NRC codes, `no_std` | MIT/Apache-2.0 |
| `socketcan-isotp` | ISO-TP (ISO 15765-2) for Linux SocketCAN | MIT/Apache-2.0 |
| `evalexpr` | Runtime formula evaluation for PID decode | MIT |
| `canparse` / `dbc` | DBC file parsing | Check license |

### Key Architectural Patterns to Adopt

1. **Data-driven diagnostics** (from AndrOBD): PIDs, conversions, DTC descriptions as external
   data files — add new parameters without recompiling
2. **Poll list pattern** (from OVMS): Static arrays defining what to poll, at what intervals,
   per vehicle state — directly applicable to our ECU profile system
3. **State-dependent polling** (from OVMS): Reduce CAN bus traffic when vehicle is off/sleeping
4. **Session recording/replay** (from OBDium): Record real CAN responses to JSON, replay in
   tests without hardware — excellent for CI
5. **Bit-level PID extraction** (from AndrOBD): offset, length, mask per logical value within
   a multi-byte PID response
