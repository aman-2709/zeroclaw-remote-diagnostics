-- Telemetry readings — designed for TimescaleDB hypertable conversion.
-- Run `SELECT create_hypertable('telemetry_readings', 'time');` after
-- installing the TimescaleDB extension.

CREATE TABLE IF NOT EXISTS telemetry_readings (
    time            TIMESTAMPTZ NOT NULL,
    device_id       TEXT NOT NULL REFERENCES devices(device_id),
    metric_name     TEXT NOT NULL,
    value_numeric   DOUBLE PRECISION,
    value_text      TEXT,
    value_json      JSONB,
    unit            TEXT,
    source          TEXT NOT NULL DEFAULT 'system'
);

-- Standard indexes (TimescaleDB will optimize further with chunks).
CREATE INDEX idx_telemetry_device_time ON telemetry_readings (device_id, time DESC);
CREATE INDEX idx_telemetry_source ON telemetry_readings (source);
CREATE INDEX idx_telemetry_metric ON telemetry_readings (metric_name);
