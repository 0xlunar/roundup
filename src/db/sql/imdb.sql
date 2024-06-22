CREATE TABLE IF NOT EXISTS imdb
(
    id                  TEXT        NOT NULL PRIMARY KEY,
    title               TEXT        NOT NULL,
    year                BIGINT      NOT NULL DEFAULT 0,
    image_url           TEXT        NOT NULL,
    rating              TEXT        NOT NULL DEFAULT 'TBD',
    runtime             BIGINT               DEFAULT NULL,
    video_thumbnail_url TEXT                 DEFAULT NULL,
    video_url           TEXT                 DEFAULT NULL,
    plot                TEXT                 DEFAULT NULL,
    popularity_rank     INTEGER              DEFAULT NULL,
    release_order       INTEGER              DEFAULT NULL,
    _type               item_type   NOT NULL,
    watchlist           BOOLEAN     NOT NULL DEFAULT FALSE,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);