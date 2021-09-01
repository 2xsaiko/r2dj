ALTER TABLE track_provider
    DROP CONSTRAINT one_of_sources;

ALTER TABLE track_provider
    ADD COLUMN source TEXT;

ALTER TABLE track_provider
    ADD COLUMN type track_provider_type;

UPDATE track_provider
SET source = youtube_id,
    type   = 'youtube'
WHERE youtube_id IS NOT NULL;

UPDATE track_provider
SET source = spotify_id,
    type   = 'spotify'
WHERE spotify_id IS NOT NULL;

UPDATE track_provider
SET source = url,
    type   = 'url'
WHERE url IS NOT NULL;

UPDATE track_provider
SET source = local_path,
    type   = 'local'
WHERE local_path IS NOT NULL;

ALTER TABLE track_provider
    DROP COLUMN youtube_id;

ALTER TABLE track_provider
    DROP COLUMN spotify_id;

ALTER TABLE track_provider
    DROP COLUMN url;

ALTER TABLE track_provider
    DROP COLUMN local_path;

ALTER TABLE track_provider
    ALTER COLUMN source SET NOT NULL;

ALTER TABLE track_provider
    ALTER COLUMN type SET NOT NULL;

---

ALTER TABLE playlist
    DROP CONSTRAINT one_of_sources;

ALTER TABLE playlist
    ADD COLUMN external_source_type external_source;

ALTER TABLE playlist
    ADD COLUMN external_source TEXT;

UPDATE playlist
SET external_source      = youtube_id,
    external_source_type = 'youtube'
WHERE youtube_id IS NOT NULL;

UPDATE playlist
SET external_source      = spotify_id,
    external_source_type = 'spotify'
WHERE spotify_id IS NOT NULL;

ALTER TABLE playlist
    DROP COLUMN youtube_id;

ALTER TABLE playlist
    DROP COLUMN spotify_id;

