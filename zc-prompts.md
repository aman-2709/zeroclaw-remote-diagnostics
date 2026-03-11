# ZeroClaw Fleet Command — Prompt Reference

All the natural-language prompts an operator can send from the dashboard. Prompts are case-insensitive and matched generously — these are examples, not exact templates.

---

## Vehicle Diagnostics (CAN Bus / OBD-II)

### Diagnostic Trouble Codes (DTCs)

```
read DTCs
get diagnostic trouble codes
check engine codes
show fault codes
any trouble codes?
```

### Vehicle Identification Number (VIN)

```
read VIN
get VIN
what is the VIN?
show VIN number
vehicle identification number
```

### Freeze Frame Data

```
read freeze frame data
show freeze frame
freeze data snapshot
read freeze
snapshot data
```

### OBD-II Sensor Readings (PIDs)

**Engine RPM**
```
read RPM
get engine speed
show engine RPM
what is the RPM?
check RPM
```

**Vehicle Speed**
```
read vehicle speed
get speed
show speed
what is the vehicle speed?
check speed
```

**Coolant Temperature**
```
read coolant temp
check coolant temperature
get engine temp
show coolant temp
what is the coolant temperature?
```

**Throttle Position**
```
show throttle position
read throttle
get throttle position
what is the throttle position?
check throttle
```

**Fuel Level**
```
read fuel level
get fuel level
show fuel
what is the fuel level?
check fuel
```

**Engine Load**
```
read engine load
get load
show engine load
what is the engine load?
check load
```

**Intake Air Temperature**
```
read intake temp
get intake air temperature
show intake temp
what is the intake air temp?
```

**Timing Advance**
```
read timing advance
get timing
show timing advance
what is the timing?
```

**Raw PID by Hex**
```
read pid 0x0C
read pid 0x0D
read pid 0x05
read pid 0x2F
read pid 12
```

### CAN Bus Monitoring

```
monitor CAN bus traffic
sniff CAN bus for 30 seconds
capture CAN traffic
monitor CAN bus
can bus traffic
bus monitor
sniff CAN 15s
```

### CAN Interface Status

```
show CAN interface state
what is the CAN bitrate?
CAN bus state
CAN status
CAN details
CAN interface
```

### Hella UDS — Read DTCs (BCR / BCF)

```
read BCR DTCs
BCR diagnostics
BCR fault codes
BCR trouble codes
read BCF DTCs
BCF diagnostics
hella DTCs
```

### Hella UDS — Read DIDs (BCR / BCF)

```
BCR voltage
BCR brake light status
BCR power supply
BCR sensor data
read BCR data
BCR status
BCF voltage
BCF data
```

### Hella UDS — Session Control (BCR / BCF)

```
BCR extended session
BCR default session
BCR tester present
BCR keep alive
BCF extended session
BCF tester present
```

---

## Log Analysis

### Search Logs

```
search logs for connection timeout
search logs for OOM
grep logs for segfault
find in logs authentication failure
search logs
```

### Error Analysis

```
analyze errors in the logs
error analysis
what errors are there?
find errors
show errors
```

### Log Statistics

```
show log statistics
log stats
log summary
log overview
show stats
```

### Tail / Recent Logs

```
tail logs
show recent logs
latest logs
last logs
show recent logs 200
tail logs 100
```

### Systemd Journal Queries

```
show journal for nginx.service
journalctl sshd
service logs for docker.service
show systemd logs
journal for mosquitto
journalctl NetworkManager
service log for zeroclaw.service
```

---

## System Information

### CPU & Processes

```
show cpu usage
which application is consuming lot of CPU?
highest cpu usage
what is consuming cpu?
memory hog processes
what processes are running?
show running processes
what's running?
show cpu info
processor info
lscpu
```

### Memory

```
show memory usage
how much RAM is available?
free memory
how much memory is left?
show RAM
```

### Disk & Storage

```
how much disk space is left?
show disk usage
free space
show storage
what is the directory size of logs?
folder size
disk usage by directory
show block devices
list disk partitions
lsblk
show partitions
```

### Temperature & Sensors

```
what is the CPU temperature?
show CPU temp
processor temperature
what is the GPU temperature?
GPU temp
show hardware sensor readings
read temperature sensor data
voltage sensor
show sensors
```

### Network

```
what is the IP address of this machine?
show IP address
network interfaces
network info
show open ports
what ports are listening?
active connections
network connections
netstat
show sockets
check network latency
run a ping test
ping test
internet latency
latency test
what is the wifi signal strength?
show wireless info
wifi info
signal strength
show ethernet info
ethtool
NIC info
```

### System Identity & Info

```
what is the uptime?
show uptime
what is the kernel version?
show kernel
uname
what is the hostname?
show hostname
what is the machine id?
show device identifier
device id
what is the product name?
show board vendor
hardware model
board info
DMI info
what device model is this board?
show device tree model
ARM model
what board is this?
```

### Date, Time & User

```
what time is it?
show current date
current time
what date is it?
whoami
who am I?
current user
logged in as
```

### Kernel Messages

```
show kernel messages
kernel log
run dmesg
boot messages
kernel ring buffer
```

### Services

```
show service status
systemctl status
which services are running?
list running services
service running
```

---

## GPS & Location

```
what is the GPS location?
show GPS coordinates
where is the device?
where is this device?
device location
current location
show coordinates
latitude and longitude
GPS fix
where am I?
```

---

## Conversational (No Execution)

These are handled as conversational replies — the device responds but doesn't run any command.

```
hello
hi, how are you?
hey there
what can you do?
what are your capabilities?
help
who are you?
how are you doing?
good morning
thanks
thank you
goodbye
```

---

## Tips

- **Case-insensitive**: "READ DTCS", "read dtcs", "Read DTCs" all work.
- **Casual language works**: "what's the temp?", "any trouble codes?", "how much space is left?"
- **Tiered inference**: Known patterns resolve locally in <1 ms. Unrecognized queries fall back to Bedrock (cloud LLM) which can interpret more creative phrasing.
- **Shell commands are read-only**: The device enforces a strict allowlist. Destructive commands (rm, dd, sudo, reboot, etc.) are always blocked.
- **No pipes or redirects**: Commands like `top | head` won't work — use flags instead. The system strips shell metacharacters automatically.
- **Durations**: For CAN monitoring you can specify duration: "monitor CAN for 30 seconds", "capture CAN 15s".
- **Line counts**: For log tailing you can specify count: "show recent logs 200", "tail logs 100".
- **Service names**: For journal queries specify the service: "journal for nginx.service", "journalctl sshd".
- **PID by hex**: You can request any OBD-II PID directly: "read pid 0x2F", "read pid 12".
