-- Write SQL here that undoes the changes done in apply.sql, back to the
-- previous migration point.

DROP TABLE playlist_entry;
DROP TABLE playlist;
DROP TABLE track_provider;
DROP TABLE album_track;
DROP TABLE album;
DROP TABLE genre;
DROP TABLE track_artist;
DROP TABLE artist;
DROP TABLE track;

DROP FUNCTION playlist_entry_before_row();
DROP TYPE external_source;
DROP TYPE track_provider_type;