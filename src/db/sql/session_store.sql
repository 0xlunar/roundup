CREATE TABLE IF NOT EXISTS session_store (
    id UUID PRIMARY KEY NOT NULL DEFAULT gen_random_uuid(),
    state json NOT NULL,
    ttl DOUBLE PRECISION NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
)