CREATE TABLE IF NOT EXISTS moviedb (
    id INTEGER PRIMARY KEY NOT NULL,
    imdb_id TEXT NOT NULL,
    title TEXT NOT NULL,
    plot TEXT NOT NULL,
    release_date DATE NOT NULL,
    image_url TEXT,
    video_id TEXT,
    certification TEXT,
    runtime BIGINT,
    popularity_rank BIGINT,
    _type item_type NOT NULL,
    watchlist BOOLEAN,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);