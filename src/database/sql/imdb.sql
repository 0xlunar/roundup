CREATE TABLE IF NOT EXISTS imdb
(
    id              VARCHAR(10) PRIMARY KEY NOT NULL,
    title           TEXT                    NOT NULL,
    year            BIGINT                  NOT NULL,
    image_url       TEXT                             DEFAULT NULL,
    _type           media_type              NOT NULL,
    plot            TEXT                    NOT NULL DEFAULT '',
    runtime_seconds BIGINT                  NOT NULL DEFAULT 0,
    video_url       TEXT                             DEFAULT NULL,
    release_date    TIMESTAMPTZ                      DEFAULT NULL,
    seasons         JSONB                            DEFAULT NULL,
    ranking         BIGINT                  NOT NULL DEFAULT 0,
    updated_at      TIMESTAMPTZ             NOT NULL DEFAULT now()
);