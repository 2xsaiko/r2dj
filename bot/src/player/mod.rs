use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures::StreamExt;
use log::{debug, error, warn};
use petgraph::graph::NodeIndex;
use pin_project_lite::pin_project;
use tokio::sync::broadcast;
use tokio::time::Duration;
use uuid::Uuid;

use audiopipe::{AudioSource, Core};
use msgtools::{proxy, Ac};
use player2x::ffplayer::{Player, PlayerEvent};
use playlistv2::treepath::TreePathBuf;
pub use playlistv2::*;

use crate::db::entity::{Playlist, Track};

// mod playlist;
mod playlistv2;
mod track;

proxy! {
    pub proxy Room1 {
        pub async fn play();
        pub async fn pause();
        pub async fn next();
        pub async fn toggle_random() -> bool;
        pub async fn add_to_queue(track: Track);
        pub async fn set_playlist(playlist: Ac<Playlist>);
        pub async fn playlist() -> Ac<Playlist>;
        pub async fn add_playlist(playlist: Ac<Playlist>, path: TreePathBuf) -> bool;
    }
}

pub struct Room {
    id: Uuid,
    tx: Room1,
    event_tx: broadcast::Sender<Event>,
}

struct RoomService {
    player: Option<Player<AudioSource>>,
    player_receiver: Option<broadcast::Receiver<PlayerEvent>>,
    audio_out: NodeIndex,
    ac: Arc<Core>,
    event_tx: broadcast::Sender<Event>,
    mode: PlayMode,
    playlist: PlaylistTracker,
    track_state: Option<TrackState>,
    clients: Vec<Client>,
}

pub enum PlayMode {
    Once,
    Repeat,
    RepeatOne,
}

pub enum Client {
    MumbleClient,
}

struct TrackState {
    track: Track,
    offset: Duration,
}

impl Room {
    pub fn new(audio_out: NodeIndex, ac: Arc<Core>) -> Self {
        let (event_tx, _) = broadcast::channel(20);

        let rd = RoomService {
            player: None,
            player_receiver: None,
            audio_out,
            ac,
            event_tx: event_tx.clone(),
            mode: PlayMode::Repeat,
            playlist: PlaylistTracker::new(Ac::new(Playlist::new())),
            track_state: None,
            clients: vec![],
        };

        let (tx, rx) = Room1::channel();

        tokio::spawn(run_room(rd, rx));

        let r = Room {
            id: Uuid::new_v4(),
            tx,
            event_tx,
        };

        r
    }

    pub fn proxy(&self) -> &Room1 {
        &self.tx
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.event_tx.subscribe()
    }
}

impl RoomService {
    fn next(&mut self) -> Option<Track> {
        // TODO song queuing
        self.playlist.next().map(|x| x.clone()).ok()
    }

    async fn skip(&mut self) {
        if let Some(player) = self.player.take() {
            // TODO: remove audio output from ac
            player.pause().await;
        }

        let tr = self.next();

        if let Some(tr) = tr {
            let path = tr.providers().first().unwrap().media_path().await.unwrap();
            let out = self.ac.add_input_to(Some(self.audio_out));
            let player = Player::new(path, out).unwrap();
            self.player_receiver = Some(player.event_listener());

            player.play().await;

            let length = player.length();

            self.player = Some(player);

            let _ = self.event_tx.send(Event::TrackChanged(tr, length));
        } else {
            let _ = self.event_tx.send(Event::TrackCleared);
        }
    }
}

async fn run_room(mut data: RoomService, mut rx: Room1Receiver) {
    loop {
        let mut player_receiver = data.player_receiver.take();
        let player_fut = FutureOption::new(player_receiver.as_mut().map(|el| el.recv()));

        tokio::select! {
            msg = rx.next() => {
                let msg = match msg {
                    None => break, // other end hung up, close the room
                    Some(msg) => msg,
                };

                match msg {
                    Room1Message::Play { callback } => {
                        match &data.player {
                            None => data.skip().await,
                            Some(pl) => pl.play().await,
                        }

                        let _ = callback.send(());
                    }
                    Room1Message::Pause { callback } => {
                        if let Some(player) = &data.player {
                            player.pause().await;
                        }

                        let _ = callback.send(());
                    }
                    Room1Message::Next { callback } => {
                        data.skip().await;
                        let _ = callback.send(());
                    }
                    Room1Message::ToggleRandom { callback } => {
                        let new_random = !data.playlist.random();
                        data.playlist.set_random(new_random);
                        let _ = callback.send(new_random);
                    }
                    Room1Message::AddToQueue { track, callback } => {
                        warn!("AddToQueue unimplemented");
                        let _ = callback.send(());
                    }
                    Room1Message::SetPlaylist { playlist, callback } => {
                        data.playlist = PlaylistTracker::new(playlist);
                        data.skip().await;
                        let _ = callback.send(());
                    }
                    Room1Message::Playlist { callback } => {
                        let _ = callback.send(data.playlist.playlist().clone());
                    }
                    Room1Message::AddPlaylist { playlist, path, callback } => {
                        let success = data.playlist.add_playlist(playlist.into_inner(), path).is_ok();
                        let _ = callback.send(success);
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
pub enum Event {
    PlayerEvent(PlayerEvent),
    TrackChanged(Track, Duration),
    TrackCleared,
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
