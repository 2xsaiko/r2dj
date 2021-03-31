-- Write SQL here that applies the changes to the database you want, starting
-- from the previous migration point.

-- Track/album metadata

CREATE TABLE track
(
    id           uuid NOT NULL,
    title        text,
    genre        uuid,
    release_date date,
    PRIMARY KEY (id)
);

CREATE TABLE artist
(
    id   uuid NOT NULL,
    name text,
    PRIMARY KEY (id)
);

CREATE TABLE track_artist
(
    track  uuid NOT NULL,
    artist uuid NOT NULL,
    PRIMARY KEY (track, artist)
);

CREATE TABLE genre
(
    id   uuid NOT NULL,
    name text,
    PRIMARY KEY (id)
);

CREATE TABLE album
(
    id           uuid NOT NULL,
    name         text,
    release_date date,
    PRIMARY KEY (id)
);

CREATE TABLE album_track
(
    album        uuid NOT NULL,
    track        uuid NOT NULL,
    track_number int,
    PRIMARY KEY (album, track)
);

-- Track providers

CREATE TABLE track_provider_local
(
    id    uuid NOT NULL,
    track uuid NOT NULL,
    path  text NOT NULL,
    PRIMARY KEY (id),
    FOREIGN KEY (track) REFERENCES track (id)
);

CREATE TABLE track_provider_url
(
    id    uuid NOT NULL,
    track uuid NOT NULL,
    url   text NOT NULL,
    PRIMARY KEY (id),
    FOREIGN KEY (track) REFERENCES track (id)
);

CREATE TABLE track_provider_spotify
(
    id         uuid NOT NULL,
    track      uuid NOT NULL,
    spotify_id text NOT NULL,
    PRIMARY KEY (id),
    FOREIGN KEY (track) REFERENCES track (id)
);

-- Playlist data

CREATE TYPE external_source AS ENUM ('spotify', 'youtube');

CREATE TABLE playlist
(
    id                   uuid NOT NULL,
    title                text NOT NULL,
    external_source_type external_source,
    external_source      text,
    created              date NOT NULL,
    modified             date,
    PRIMARY KEY (id)
);

CREATE TABLE playlist_entry
(
    id           uuid NOT NULL,
    playlist     uuid NOT NULL,
    index        int  NOT NULL,
    track        uuid,
    sub_playlist uuid,
    PRIMARY KEY (id),
    FOREIGN KEY (playlist) REFERENCES playlist (id),
    FOREIGN KEY (track) REFERENCES track (id),
    FOREIGN KEY (sub_playlist) REFERENCES playlist (id),
    CONSTRAINT type_check CHECK ( num_nonnulls(track, sub_playlist) = 1 )
);

CREATE INDEX by_playlist ON playlist_entry (playlist);

-- This is used to make sure that there are no duplicate track indices in the
-- playlist if a track is inserted somewhere in the middle of the playlist.
CREATE FUNCTION playlist_entry_before_row() RETURNS TRIGGER AS
$$
BEGIN
    UPDATE playlist_entry
    SET index = index + 1
    WHERE index = new.index;
    RETURN new;
END;
$$ SECURITY DEFINER LANGUAGE PLPGSQL;

CREATE TRIGGER playlist_entry_before_row
    BEFORE INSERT OR UPDATE
    ON playlist_entry
    FOR EACH ROW
EXECUTE FUNCTION playlist_entry_before_row();
