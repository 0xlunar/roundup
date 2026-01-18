CREATE TABLE IF NOT EXISTS torrent
(
    hash       TEXT PRIMARY KEY                 NOT NULL,
    imdb_id    VARCHAR(10) REFERENCES imdb (id) NOT NULL,
    season     BIGINT                                    DEFAULT NULL,
    episode    BIGINT                                    DEFAULT NULL,
    size_bytes BIGINT                           NOT NULL DEFAULT 0,
    state      TEXT                             NOT NULL,
    updated_at TIMESTAMPTZ                      NOT NULL DEFAULT now()
);