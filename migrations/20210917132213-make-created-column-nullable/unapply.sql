UPDATE playlist
SET created = to_timestamp(0)
WHERE created IS NULL;

ALTER TABLE playlist
    ALTER created SET NOT NULL;