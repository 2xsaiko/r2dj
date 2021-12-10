ALTER TABLE album
    DROP COLUMN deleted;

ALTER TABLE artist
    DROP COLUMN deleted;

ALTER TABLE genre
    DROP COLUMN deleted;

ALTER TABLE playlist
    DROP COLUMN deleted;

ALTER TABLE track
    DROP COLUMN deleted;