DO $$ BEGIN
    CREATE TYPE item_type as ENUM ('movie', 'tvshow');
EXCEPTION
    WHEN duplicate_object THEN null;
END $$;