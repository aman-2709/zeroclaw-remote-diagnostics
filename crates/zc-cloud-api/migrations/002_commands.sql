-- Command dispatch and response tracking.

CREATE TABLE IF NOT EXISTS commands (
    id              UUID PRIMARY KEY,
    fleet_id        TEXT NOT NULL,
    device_id       TEXT NOT NULL REFERENCES devices(device_id),
    natural_language TEXT NOT NULL,
    initiated_by    TEXT NOT NULL,
    correlation_id  UUID NOT NULL,
    timeout_secs    INTEGER NOT NULL DEFAULT 30,

    -- Parsed intent (nullable — filled after NL inference)
    tool_name       TEXT,
    tool_args       JSONB,
    confidence      DOUBLE PRECISION,

    -- Response (nullable — filled when device responds)
    status          TEXT NOT NULL DEFAULT 'pending',
    inference_tier  TEXT,
    response_text   TEXT,
    response_data   JSONB,
    latency_ms      BIGINT,
    responded_at    TIMESTAMPTZ,
    error           TEXT,

    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_commands_device_id ON commands (device_id);
CREATE INDEX idx_commands_status ON commands (status);
CREATE INDEX idx_commands_created_at ON commands (created_at DESC);
CREATE INDEX idx_commands_correlation_id ON commands (correlation_id);
