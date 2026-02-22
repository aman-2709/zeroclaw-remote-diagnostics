-- Device registry table.

CREATE TABLE IF NOT EXISTS devices (
    id              UUID PRIMARY KEY,
    fleet_id        UUID NOT NULL,
    device_id       TEXT NOT NULL UNIQUE,
    status          TEXT NOT NULL DEFAULT 'provisioning',
    vin             TEXT,
    hardware_type   TEXT NOT NULL DEFAULT 'unknown',
    certificate_id  TEXT,
    last_heartbeat  TIMESTAMPTZ,
    metadata        JSONB NOT NULL DEFAULT '{}',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_devices_fleet_id ON devices (fleet_id);
CREATE INDEX idx_devices_status ON devices (status);
