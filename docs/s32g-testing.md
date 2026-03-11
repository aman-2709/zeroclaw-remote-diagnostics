# S32G Hardware Testing

## Device: NXP S32G274A (TeraOBD2 Dongle)

| Field | Value |
|-------|-------|
| Hostname | ubuntu-s32g274ateraobd2dongleubuntu |
| Arch | aarch64 (ARM64) |
| OS | Linux 5.10.41-rt42 (PREEMPT_RT) |
| IP | 192.168.62.47 |
| User | bluebox |
| Password | bluebox |
| Device ID | s32g-001 |
| Fleet ID | fleet-alpha |

## Quick Connect

```bash
# SSH
sshpass -p 'bluebox' ssh -o StrictHostKeyChecking=no bluebox@192.168.62.47

# SCP
sshpass -p 'bluebox' scp -o StrictHostKeyChecking=no <file> bluebox@192.168.62.47:<dest>

# Run a remote command
sshpass -p 'bluebox' ssh -o StrictHostKeyChecking=no bluebox@192.168.62.47 '<command>'

# Sudo on device (pipe password)
echo bluebox | sudo -S <command>
```

## Agent Deployment

| Path | Description |
|------|-------------|
| `/opt/zeroclaw/` | Install root |
| `/opt/zeroclaw/zc-fleet-agent` | Agent binary |
| `/opt/zeroclaw/agent.toml` | Agent config |
| `/opt/zeroclaw/target/release/` | Build output |
| `/tmp/zc-agent.log` | Runtime logs (when started manually) |

### agent.toml

```toml
fleet_id = "fleet-alpha"
device_id = "s32g-001"
can_interface = "can0"
heartbeat_interval_secs = 10
shadow_sync_interval_secs = 30
log_paths = ["/var/log/syslog"]

[mqtt]
broker_host = "192.168.62.111"
broker_port = 1883
client_id = "s32g-001"
use_tls = false

[ollama]
host = "http://localhost:11434"
model = "phi3:mini"
timeout_secs = 30
enabled = false
```

## CAN Bus

- **Interface**: `can0` — UP at 250 kbps
- **Connected ECU**: Hella BCR (Body Controller Rear)
- **BCR request ID**: 0x60D
- **BCR response ID**: 0x58D
- **Mode**: Read-only (UDS write services blocked by safety layer)

## Build & Deploy

```bash
# 1. Create source tarball (on dev machine)
cd /home/xl4/dev/personal-projects/zeroclaw-remote-diagnostics
tar czf /tmp/zc-source.tar.gz \
  --exclude='target' --exclude='.git' --exclude='node_modules' \
  --exclude='frontend/build' --exclude='frontend/.svelte-kit' .

# 2. Upload to device
sshpass -p 'bluebox' scp -o StrictHostKeyChecking=no \
  /tmp/zc-source.tar.gz bluebox@192.168.62.47:/opt/zeroclaw/zc-source.tar.gz

# 3. Extract on device
sshpass -p 'bluebox' ssh -o StrictHostKeyChecking=no bluebox@192.168.62.47 \
  'cd /opt/zeroclaw && tar xzf zc-source.tar.gz'

# 4. Build on device (~8-12 min full build, incremental faster)
#    IMPORTANT: must source cargo env first
sshpass -p 'bluebox' ssh -o StrictHostKeyChecking=no bluebox@192.168.62.47 \
  'source $HOME/.cargo/env && cd /opt/zeroclaw && cargo build --release -p zc-fleet-agent'

# 5. Stop old agent
sshpass -p 'bluebox' ssh -o StrictHostKeyChecking=no bluebox@192.168.62.47 \
  'echo bluebox | sudo -S bash -c "ps aux | grep zc-fleet-agent | grep -v grep | awk \"{print \\\$2}\" | xargs -r kill -9"'

# 6. Deploy new binary
sshpass -p 'bluebox' ssh -o StrictHostKeyChecking=no bluebox@192.168.62.47 \
  'rm -f /opt/zeroclaw/zc-fleet-agent && cp /opt/zeroclaw/target/release/zc-fleet-agent /opt/zeroclaw/zc-fleet-agent'

# 7. Start new agent
sshpass -p 'bluebox' ssh -o StrictHostKeyChecking=no bluebox@192.168.62.47 \
  'echo bluebox | sudo -S bash -c "rm -f /tmp/zc-agent.log; RUST_LOG=info nohup /opt/zeroclaw/zc-fleet-agent /opt/zeroclaw/agent.toml > /tmp/zc-agent.log 2>&1 &"'
```

## Monitoring & Troubleshooting

```bash
# Watch build progress
sshpass -p 'bluebox' ssh -o StrictHostKeyChecking=no bluebox@192.168.62.47 \
  'source $HOME/.cargo/env && cd /opt/zeroclaw && watch -n5 "ls target/release/zc-fleet-agent 2>/dev/null && echo DONE || (ls target/release/deps/*.d | wc -l; echo crates compiled)"'

# Check agent process (should be exactly 1)
sshpass -p 'bluebox' ssh -o StrictHostKeyChecking=no bluebox@192.168.62.47 \
  'ps aux | grep zc-fleet | grep -v grep'

# Read agent logs
sshpass -p 'bluebox' ssh -o StrictHostKeyChecking=no bluebox@192.168.62.47 \
  'echo bluebox | sudo -S tail -30 /tmp/zc-agent.log 2>/dev/null'

# Check CAN bus status
sshpass -p 'bluebox' ssh -o StrictHostKeyChecking=no bluebox@192.168.62.47 \
  'ip -d link show can0'

# Monitor raw CAN traffic
sshpass -p 'bluebox' ssh -o StrictHostKeyChecking=no bluebox@192.168.62.47 \
  'candump can0'

# Test MQTT connectivity from device
sshpass -p 'bluebox' ssh -o StrictHostKeyChecking=no bluebox@192.168.62.47 \
  'timeout 3 bash -c "echo > /dev/tcp/192.168.62.111/1883" && echo "MQTT OK" || echo "MQTT FAIL"'

# Listen for heartbeats (on dev machine)
timeout 15 mosquitto_sub -h localhost -p 1883 -t "fleet/fleet-alpha/s32g-001/heartbeat/#" -v

# Listen for shadow updates (on dev machine)
timeout 35 mosquitto_sub -h localhost -p 1883 -t "fleet/fleet-alpha/s32g-001/shadow/#" -v

# Listen for command responses (on dev machine)
mosquitto_sub -h localhost -p 1883 -t "fleet/fleet-alpha/s32g-001/response/#" -v
```

## Dev Machine (MQTT Broker)

| Service | Address |
|---------|---------|
| Mosquitto | 192.168.62.111:1883 (plaintext) |
| Cloud API | 192.168.62.111:3002 |
| Frontend | 192.168.62.111:5173 (vite dev) |

## Known Issues

- **LTO link is slow**: `cargo build --release` with `lto = true` + `codegen-units = 1` takes ~3-5 min for the final link step on ARM. Be patient.
- **Duplicate processes**: If multiple agent instances start with the same MQTT client_id, mosquitto kicks them in a loop. Always verify `pgrep -c zc-fleet-agent` shows exactly 1.
- **Sudo via SSH**: Use `echo bluebox | sudo -S bash -c "..."` pattern. Plain `sudo` fails without a TTY.
- **Rust toolchain**: Must `source $HOME/.cargo/env` before any cargo/rustc command on the device.
- **Log file permissions**: `/tmp/zc-agent.log` may be owned by root if agent was started with sudo. Use `sudo -S cat` or `sudo -S tail` to read.
