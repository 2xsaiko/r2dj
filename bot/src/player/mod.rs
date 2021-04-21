use log::error;
use tokio::sync::{broadcast, mpsc};
use tokio::time::Duration;
use uuid::Uuid;

use audiopipe::mixer::MixerInput;
use player2x::ffplayer::{Player, PlayerEvent};
pub use playlist::*;

use crate::player::track::Track;

pub mod import;
mod playlist;
mod track;

struct RoomData {
    mode: PlayMode,
    playlist: Playlist,
    track_state: Option<TrackState>,
    player: Option<Player<MixerInput>>,
    audio_out: MixerInput,
}

pub struct Room {
    id: Uuid,
    clients: Vec<Client>,
    tx: mpsc::Sender<Message>,
}

pub enum Client {
    MumbleClient,
}

struct TrackState {
    track: Track,
    offset: Duration,
}

impl Room {
    pub fn new(audio_out: MixerInput) -> Self {
        let rd = RoomData {
            mode: PlayMode::Repeat,
            playlist: Playlist::new(),
            track_state: None,
            player: None,
            audio_out,
        };

        let (tx, rx) = mpsc::channel(20);

        tokio::spawn(run_room(rd, rx));

        let r = Room {
            id: Uuid::new_v4(),
            clients: vec![],
            tx,
        };

        r
    }

    pub async fn play(&self) {
        self.tx.send(Message::Play).await.unwrap();
    }

    pub async fn pause(&self) {
        self.tx.send(Message::Pause).await.unwrap();
    }

    pub async fn next(&self) {
        self.tx.send(Message::Next).await.unwrap();
    }

    pub async fn add_to_queue(&self, track: Track) {
        self.tx.send(Message::Queue(track)).await.unwrap();
    }

    pub async fn set_playlist(&self, playlist: Playlist) {
        self.tx.send(Message::SetPlaylist(playlist)).await.unwrap();
    }
}

impl RoomData {
    fn get_next(&mut self) -> Track {
        // TODO song queuing
        self.playlist.next()
    }

    async fn skip(&mut self) {
        if let Some(player) = self.player.take() {
            player.pause().await;
        }

        let tr = self.get_next();
        let path = tr.providers().first().unwrap().media_path().await.unwrap();
        let player = Player::new(path, self.audio_out.clone()).unwrap();
        player.play().await;
        self.player = Some(player);
    }
}

async fn run_room(mut data: RoomData, mut rx: mpsc::Receiver<Message>) {
    let (_dummy_tx, dummy_rx) = broadcast::channel(0);
    let mut player_listener = data
        .player
        .as_ref()
        .map(|p| p.event_listener())
        .unwrap_or(dummy_rx);
    loop {
        tokio::select! {
            msg = rx.recv() => {
                let msg = match msg {
                    None => break, // other end hung up, close the room
                    Some(msg) => msg,
                };

                match msg {
                    Message::Play => {
                        match &data.player {
                            None => data.skip().await,
                            Some(pl) => pl.play().await,
                        }
                    }
                    Message::Pause => {
                        if let Some(player) = &data.player {
                            player.pause().await;
                        }
                    }
                    Message::Next => {
                        data.skip().await;
                    }
                    Message::Queue(t) => {
                        todo!()
                    }
                    Message::SetPlaylist(pl) => {
                        data.playlist = pl;
                        data.skip().await;
                    }
                }
            }
            ev = player_listener.recv() => {
                match ev {
                    Ok(PlayerEvent::Playing { ..}) => {}
                    Ok(PlayerEvent::Paused { stopped, ..}) => {
                        if stopped {
                            data.skip().await;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        // not sure this can happen, but I guess we should play
                        // the next song?
                        data.skip().await;
                    }
                    Err(x) => {
                        error!("error receiving player events: {}", x);
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
enum Message {
    Play,
    Pause,
    Next,
    Queue(Track),
    SetPlaylist(Playlist),
}
