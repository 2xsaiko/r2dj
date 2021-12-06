ALTER TABLE track
    DROP CONSTRAINT track_code_not_empty;

ALTER TABLE playlist
    DROP CONSTRAINT playlist_code_not_empty;

ALTER TABLE album
    DROP CONSTRAINT album_code_not_empty;

ALTER TABLE artist
    DROP CONSTRAINT artist_code_not_empty;

ALTER TABLE genre
    DROP CONSTRAINT genre_code_not_empty;

ALTER TABLE track
    DROP CONSTRAINT track_code_key;

ALTER TABLE playlist
    DROP CONSTRAINT playlist_code_key;

ALTER TABLE album
    DROP CONSTRAINT album_code_key;

ALTER TABLE artist
    DROP CONSTRAINT artist_code_key;

ALTER TABLE genre
    DROP CONSTRAINT genre_code_key;

ALTER TABLE track
    DROP COLUMN code;

ALTER TABLE playlist
    DROP COLUMN code;

ALTER TABLE album
    DROP COLUMN code;

ALTER TABLE artist
    DROP COLUMN code;

ALTER TABLE genre
    DROP COLUMN code;

DROP SEQUENCE track_code_seq;
DROP SEQUENCE playlist_code_seq;
DROP SEQUENCE album_code_seq;
DROP SEQUENCE artist_code_seq;
DROP SEQUENCE genre_code_seq;