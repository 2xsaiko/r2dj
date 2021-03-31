use std::path::PathBuf;

use tokio::time::Duration;
use uuid::Uuid;

use crate::player::playlist::{PlayMode, Playlist};

mod playlist;

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

#[derive(Clone, Debug)]
pub struct Track {
    path: PathBuf,
}
