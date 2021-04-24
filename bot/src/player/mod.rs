use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use log::error;
use pin_project_lite::pin_project;
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
    player_receiver: Option<broadcast::Receiver<PlayerEvent>>,
    audio_out: MixerInput,
    event_tx: broadcast::Sender<Event>,
}

pub struct Room {
    id: Uuid,
    clients: Vec<Client>,
    tx: mpsc::Sender<Message>,
    event_tx: broadcast::Sender<Event>,
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
        let (event_tx, _) = broadcast::channel(20);

        let rd = RoomData {
            mode: PlayMode::Repeat,
            playlist: Playlist::new(),
            track_state: None,
            player: None,
            player_receiver: None,
            audio_out,
            event_tx: event_tx.clone(),
        };

        let (tx, rx) = mpsc::channel(20);

        tokio::spawn(run_room(rd, rx));

        let r = Room {
            id: Uuid::new_v4(),
            clients: vec![],
            tx,
            event_tx,
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

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.event_tx.subscribe()
    }
}

impl RoomData {
    fn get_next(&mut self) -> Track {
        // TODO song queuing
        self.playlist.next()
    }

    async fn skip(&mut self) {
        let playing = if let Some(player) = self.player.take() {
            let p = player.is_playing().await;
            player.pause().await;
            p
        } else {
            false
        };

        let tr = self.get_next();
        let path = tr.providers().first().unwrap().media_path().await.unwrap();
        let player = Player::new(path, self.audio_out.clone()).unwrap();
        self.player_receiver = Some(player.event_listener());

        if playing {
            player.play().await;
        }

        let length = player.length();

        self.player = Some(player);

        let _ = self.event_tx.send(Event::TrackChanged(tr, length));
    }
}

async fn run_room(mut data: RoomData, mut rx: mpsc::Receiver<Message>) {
    loop {
        let mut player_receiver = data.player_receiver.take();
        let player_fut = FutureOption::new(player_receiver.as_mut().map(|el| el.recv()));

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
                    Message::Queue(_t) => {
                        todo!()
                    }
                    Message::SetPlaylist(pl) => {
                        data.playlist = pl;
                        data.skip().await;
                    }
                }
            }
            ev = player_fut => {
                match ev {
                    Ok(ev) => {
                        match ev {
                            PlayerEvent::Playing { .. } => {}
                            PlayerEvent::Paused { stopped, .. } => {
                                if stopped {
                                    data.skip().await;
                                }
                            }
                        }

                        let _ = data.event_tx.send(Event::PlayerEvent(ev));
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

        // give player_receiver back to data unless it's already got a new one
        // (in case the track changed)
        data.player_receiver = data.player_receiver.or(player_receiver);
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

#[derive(Debug, Clone)]
pub enum Event {
    PlayerEvent(PlayerEvent),
    TrackChanged(Track, Duration),
}

pin_project! {
    #[derive(Debug, Clone, Copy)]
    struct FutureOption<T> {
        #[pin]
        inner: Option<T>,
    }
}

impl<T> FutureOption<T> {
    pub fn new(inner: Option<T>) -> Self {
        FutureOption { inner }
    }
}

impl<T> Future for FutureOption<T>
where
    T: Future,
{
    type Output = T::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project().inner.as_pin_mut() {
            None => Poll::Pending,
            Some(fut) => fut.poll(cx),
        }
    }
}
