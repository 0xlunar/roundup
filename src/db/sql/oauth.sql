CREATE TABLE IF NOT EXISTS oauth (
    access_token TEXT PRIMARY KEY NOT NULL,
    refresh_token TEXT NOT NULL,
    scope TEXT[] NOT NULL,
    email TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
)