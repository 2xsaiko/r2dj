DROP TRIGGER playlist_entry_before_row ON playlist_entry;
DROP FUNCTION playlist_entry_before_row();

-- This is used to make sure that there are no duplicate track indices in the
-- playlist if a track is inserted somewhere in the middle of the playlist.
CREATE FUNCTION playlist_entry_before_row() RETURNS TRIGGER AS
$$
BEGIN
    UPDATE playlist_entry
    SET index = index + 1
    WHERE playlist = new.playlist AND index = new.index;
    RETURN new;
END;
$$ SECURITY DEFINER LANGUAGE PLPGSQL;

CREATE TRIGGER playlist_entry_before_row
    BEFORE INSERT OR UPDATE
    ON playlist_entry
    FOR EACH ROW
EXECUTE FUNCTION playlist_entry_before_row();
