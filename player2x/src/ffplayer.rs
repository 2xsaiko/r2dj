use std::ffi::OsStr;
use std::fmt::Debug;
use std::io;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dasp::{Frame, Sample};
use futures::future::BoxFuture;
use futures::{FutureExt, Sink, SinkExt};
use log::debug;
use log::error;
use thiserror::Error;
use tokio::io::AsyncReadExt;
use tokio::process::{ChildStdout, Command};
use tokio::select;
use tokio::sync::{broadcast, oneshot, Mutex};
use tokio::task::JoinHandle;

use audiopipe::AudioSource;

use crate::ffmpeg::{ffpipe, FfmpegConfig, Format, PathSource, TranscoderOutput};
use crate::ffprobe;

pub struct Player<W> {
    path: PathBuf,
    duration: Duration,
    pipe: Arc<Mutex<W>>,
    state: Arc<Mutex<State>>,
    sender: broadcast::Sender<PlayerEvent>,
}

struct State {
    position: Duration,
    playing_state: Option<PlayingState>,
    playing_tracker: Option<PlayingTracker>,
}

struct PlayingState {
    playing_since: Instant,
}

struct PlayingTracker {
    task: JoinHandle<()>,
    tx: oneshot::Sender<()>,
}

impl Player<AudioSource> {
    pub fn new<P: Into<PathBuf>>(path: P, pipe: AudioSource) -> Result<Self> {
        let path = path.into();
        let info = ffprobe::ffprobe(&path)?;

        let (tx, _) = broadcast::channel(20);

        Ok(Player {
            path,
            duration: info.duration(),
            pipe: Arc::new(Mutex::new(pipe)),
            state: Arc::new(Mutex::new(State {
                position: Duration::ZERO,
                playing_state: None,
                playing_tracker: None,
            })),
            sender: tx,
        })
    }

    pub async fn pause(&self) {
        let mut state = self.state.lock().await;

        let tracker = match state.playing_tracker.take() {
            None => return,
            Some(tracker) => tracker,
        };

        drop(state);

        tracker.tx.send(()).unwrap();
        tracker.task.await.unwrap();

        self.pipe.lock().await.set_running(false);
    }

    pub async fn is_playing(&self) -> bool {
        let state = self.state.lock().await;

        state.playing_tracker.is_some()
    }

    pub fn length(&self) -> Duration {
        self.duration
    }

    pub async fn position(&self) -> Duration {
        position(&*self.state.lock().await)
    }

    pub fn event_listener(&self) -> broadcast::Receiver<PlayerEvent> {
        self.sender.subscribe()
    }
}

impl Player<AudioSource> {
    pub async fn play(&self) {
        let mut state = self.state.lock().await;

        if state.playing_state.is_some() {
            return;
        }

        let (tx, rx) = oneshot::channel();

        let pipe = self.pipe.clone();
        let s = self.state.clone();
        let path = self.path.clone();
        let position = state.position;
        let sender = self.sender.clone();

        let now = Instant::now();

        let task = tokio::spawn(async move {
            let pipe = pipe;
            debug!("1");
            let mut pipe = pipe.lock().await;
            debug!("2");
            pipe.set_running(true);
            debug!("3");

            let _ = sender.send(PlayerEvent::Playing {
                now: Instant::now(),
                pos: position,
            });

            let r = select!(
                result = ffpipe(
                    PathSource::new(path),
                    Recoder::new(&mut *pipe),
                    FfmpegConfig::default()
                        .start_at(position)
                        .channels(2)
                        .output_format(Format::native_pcm(48000)),
                ) => match result {
                    Ok(_) => Ok(true),
                    Err(e) => Err(e),
                },
                _ = rx => Ok(false),
            );

            let mut state = s.lock().await;
            let playing_state = state.playing_state.take().unwrap();
            state.position += Instant::now().duration_since(playing_state.playing_since);
            state.playing_tracker.take();

            match r {
                Ok(stopped) => {
                    let _ = sender.send(PlayerEvent::Paused {
                        now: Instant::now(),
                        pos: state.position,
                        stopped,
                    });
                }
                Err(e) => {
                    error!("ffmpeg error: {}", e);
                    let _ = sender.send(PlayerEvent::Paused {
                        now,
                        pos: state.position,
                        stopped: false,
                    });
                }
            }
        });

        state.playing_state = Some(PlayingState { playing_since: now });
        state.playing_tracker = Some(PlayingTracker { task, tx });
    }

    pub async fn seek(&mut self, pos: Duration) {
        if self.is_playing().await {
            self.pause().await;
            self.state.lock().await.position = pos.clamp(Duration::ZERO, self.duration);
            self.play().await;
        } else {
            self.state.lock().await.position = pos.clamp(Duration::ZERO, self.duration);
        }
    }
}

fn position(state: &State) -> Duration {
    match &state.playing_state {
        None => state.position,
        Some(playing_state) => {
            state.position + Instant::now().duration_since(playing_state.playing_since)
        }
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("ffprobe error: {0}")]
    Ffprobe(#[from] ffprobe::Error),
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PlayerEvent {
    Playing {
        now: Instant,
        pos: Duration,
    },
    Paused {
        now: Instant,
        pos: Duration,
        stopped: bool,
    },
}

struct Recoder<T> {
    inner: T,
}

impl<T> Recoder<T> {
    pub fn new(inner: T) -> Self {
        Recoder { inner }
    }
}

impl<'a, T> TranscoderOutput<'a> for Recoder<T>
where
    T: Sink<[f32; 2]> + Unpin + Send + 'a,
    T::Error: Debug,
{
    fn to_arg(&self) -> &OsStr {
        OsStr::new("-")
    }

    fn pre_spawn(&self, command: &mut Command) {
        command.stdout(Stdio::piped());
    }

    fn handle_stdout(mut self, mut stdout: ChildStdout) -> BoxFuture<'a, io::Result<()>> {
        async move {
            loop {
                let mut bytes = [0; 4];

                match stdout.read_exact(&mut bytes).await {
                    Ok(_) => {}
                    Err(e) if e.kind() == ErrorKind::UnexpectedEof => break Ok(()),
                    Err(e) => break Err(e),
                }

                let data = [
                    i16::from_ne_bytes([bytes[0], bytes[1]]),
                    i16::from_ne_bytes([bytes[2], bytes[3]]),
                ];

                match self.inner.send(Frame::map(data, Sample::to_sample)).await {
                    Ok(_) => {}
                    Err(e) => {
                        break Err(io::Error::new(
                            ErrorKind::Other,
                            format!("sink error: {:?}", e),
                        ))
                    }
                }
            }
        }
        .boxed()
    }
}
