ALTER TABLE playlist
    ALTER created DROP NOT NULL;

UPDATE playlist
SET created = NULL
WHERE created = to_timestamp(0);