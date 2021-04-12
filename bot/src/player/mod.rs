use tokio::time::Duration;
use uuid::Uuid;

use crate::player::playlist::{PlayMode, Playlist};
use crate::player::track::Track;

mod playlist;
mod track;
mod import;

pub struct Room {
    id: Uuid,
    mode: PlayMode,
    playlist: Playlist,
    track_state: Option<TrackState>,
    clients: Vec<Client>,
}

pub enum Client {
    MumbleClient,
}

struct TrackState {
    track: Track,
    offset: Duration,
}
