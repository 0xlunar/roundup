DO $$ BEGIN
    CREATE TYPE user_type as ENUM ('admin', 'user');
EXCEPTION
    WHEN duplicate_object THEN null;
END $$;