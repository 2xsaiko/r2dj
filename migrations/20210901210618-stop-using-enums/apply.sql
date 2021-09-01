ALTER TABLE track_provider
    ADD COLUMN local_path TEXT;

ALTER TABLE track_provider
    ADD COLUMN url TEXT;

ALTER TABLE track_provider
    ADD COLUMN spotify_id TEXT;

ALTER TABLE track_provider
    ADD COLUMN youtube_id TEXT;

UPDATE track_provider
SET local_path = source
WHERE type = 'local';

UPDATE track_provider
SET url = source
WHERE type = 'url';

UPDATE track_provider
SET spotify_id = source
WHERE type = 'spotify';

UPDATE track_provider
SET youtube_id = source
WHERE type = 'youtube';

ALTER TABLE track_provider
    DROP COLUMN type;

ALTER TABLE track_provider
    DROP COLUMN source;

ALTER TABLE track_provider
    ADD CONSTRAINT one_of_sources CHECK ( num_nonnulls(local_path, url, spotify_id, youtube_id) = 1 );

---

ALTER TABLE playlist
    ADD COLUMN spotify_id TEXT;

ALTER TABLE playlist
    ADD COLUMN youtube_id TEXT;

UPDATE playlist
SET spotify_id = external_source
WHERE external_source_type = 'spotify';

UPDATE playlist
SET youtube_id = external_source
WHERE external_source_type = 'youtube';

ALTER TABLE playlist
    DROP COLUMN external_source;

ALTER TABLE playlist
    DROP COLUMN external_source_type;

ALTER TABLE playlist
    ADD CONSTRAINT one_of_sources CHECK ( num_nonnulls(spotify_id, youtube_id) <= 1 );