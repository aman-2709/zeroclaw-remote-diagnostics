-- Device heartbeat log for uptime tracking and fleet health.

CREATE TABLE IF NOT EXISTS heartbeats (
    device_id       TEXT NOT NULL REFERENCES devices(device_id),
    fleet_id        TEXT NOT NULL,
    status          TEXT NOT NULL,
    uptime_secs     BIGINT NOT NULL,
    ollama_status   TEXT NOT NULL DEFAULT 'unknown',
    can_status      TEXT NOT NULL DEFAULT 'unknown',
    agent_version   TEXT NOT NULL,
    received_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_heartbeats_device_time ON heartbeats (device_id, received_at DESC);
