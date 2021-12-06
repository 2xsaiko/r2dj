CREATE SEQUENCE track_code_seq START 1;
CREATE SEQUENCE playlist_code_seq START 1;
CREATE SEQUENCE album_code_seq START 1;
CREATE SEQUENCE artist_code_seq START 1;
CREATE SEQUENCE genre_code_seq START 1;

ALTER TABLE track
    ADD COLUMN code TEXT NOT NULL DEFAULT to_char(nextval('track_code_seq'::regclass), '00000000');

ALTER TABLE playlist
    ADD COLUMN code TEXT NOT NULL DEFAULT to_char(nextval('playlist_code_seq'::regclass), '00000');

ALTER TABLE album
    ADD COLUMN code TEXT NOT NULL DEFAULT to_char(nextval('album_code_seq'::regclass), '000000');

ALTER TABLE artist
    ADD COLUMN code TEXT NOT NULL DEFAULT to_char(nextval('artist_code_seq'::regclass), '00000');

ALTER TABLE genre
    ADD COLUMN code TEXT NOT NULL DEFAULT to_char(nextval('genre_code_seq'::regclass), '00000');

ALTER TABLE track
    ADD CONSTRAINT track_code_key UNIQUE (code);

ALTER TABLE playlist
    ADD CONSTRAINT playlist_code_key UNIQUE (code);

ALTER TABLE album
    ADD CONSTRAINT album_code_key UNIQUE (code);

ALTER TABLE artist
    ADD CONSTRAINT artist_code_key UNIQUE (code);

ALTER TABLE genre
    ADD CONSTRAINT genre_code_key UNIQUE (code);

ALTER TABLE track
    ADD CONSTRAINT track_code_not_empty CHECK ( code <> '' );

ALTER TABLE playlist
    ADD CONSTRAINT playlist_code_not_empty CHECK ( code <> '' );

ALTER TABLE album
    ADD CONSTRAINT album_code_not_empty CHECK ( code <> '' );

ALTER TABLE artist
    ADD CONSTRAINT artist_code_not_empty CHECK ( code <> '' );

ALTER TABLE genre
    ADD CONSTRAINT genre_code_not_empty CHECK ( code <> '' );
