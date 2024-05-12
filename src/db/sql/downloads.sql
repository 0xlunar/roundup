CREATE TABLE IF NOT EXISTS active_downloads
(
    id          SERIAL PRIMARY KEY NOT NULL,
    imdb_id     TEXT               NOT NULL,
    season      INTEGER,
    episode     INTEGER,
    quality     TEXT               NOT NULL,
    _type       item_type          NOT NULL,
    magnet_hash TEXT               NOT NULL,
    state       TEXT               NOT NULL DEFAULT 'Not Started',
    progress    FLOAT              NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ        NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ        NOT NULL DEFAULT now()
);