CREATE TABLE IF NOT EXISTS device_shadows (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    device_id TEXT NOT NULL REFERENCES devices(device_id),
    shadow_name TEXT NOT NULL,
    reported JSONB NOT NULL DEFAULT '{}',
    desired JSONB NOT NULL DEFAULT '{}',
    version BIGINT NOT NULL DEFAULT 1,
    last_updated TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(device_id, shadow_name)
);

CREATE INDEX IF NOT EXISTS idx_device_shadows_device_id ON device_shadows(device_id);
