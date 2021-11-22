use std::cmp::min;
use std::fmt;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::channel::oneshot;
use futures::{FutureExt, StreamExt};
use log::{debug, info, LevelFilter};
use simplelog::{Config, TerminalMode};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::ConnectOptions;
use tokio::time::interval;
use uuid::Uuid;

use audiopipe::Core;
use msgtools::Ac;
use mumble::{MumbleClient, MumbleConfig};
use player2x::ffplayer::PlayerEvent;

use crate::db::entity;
use crate::player::{Event as RoomEvent, Room};

const CRATE_NAME: &str = env!("CARGO_PKG_NAME");
const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

mod commands;
mod config;
mod db;
mod player;
mod spotify;

#[tokio::main]
async fn main() {
    let config = load_config();

    simplelog::TermLogger::init(
        LevelFilter::Debug,
        Config::default(),
        TerminalMode::default(),
    )
    .unwrap();

    info!("Starting {} {}", CRATE_NAME, CRATE_VERSION);

    let mut co = config
        .db_url
        .parse::<PgConnectOptions>()
        .unwrap()
        .application_name(CRATE_NAME);

    co.log_statements(LevelFilter::Trace);

    let pool = PgPoolOptions::new()
        .max_connections(config.db_pool_size)
        .min_connections(config.db_pool_size_min)
        .idle_timeout(Some(Duration::from_secs(600)))
        .connect_with(co)
        .await
        .unwrap();

    let id = Uuid::from_str("99b071f7-bdae-48b4-9c0a-aac91332c348").unwrap();
    let pl = Ac::new(
        entity::Playlist::load(id, &mut pool.acquire().await.unwrap())
            .await
            .unwrap(),
    );

    println!("{:#?}", pl);

    let mumble_config = MumbleConfig {
        username: config.name.clone(),
    };

    let ac = Arc::new(Core::new(48000));

    let client = mumble::MumbleClient::connect(
        &config.mumble_domain,
        config.mumble_port,
        config.mumble_cert,
        mumble_config,
        &ac,
    )
    .await
    .unwrap();

    let mut r = client.event_subscriber().await.unwrap();

    let room = Room::new(client.audio_input().await.unwrap(), ac);
    let mut room_events = room.subscribe();
    let _ = room.proxy().set_playlist(pl).await;

    let mut prev_rst = RoomStatus::default();
    let mut rst = RoomStatus::default();
    let mut update_timer = interval(Duration::from_secs(5));

    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let mut shutdown_rx = shutdown_rx.into_stream();

    let mut bot = Bot {
        client,
        room,
        shutdown_fuse: Some(shutdown_tx),
    };

    // let mut player = Player::new("04 - Bone Dry.mp3", client.audio_input()).unwrap();
    // player.play().await;

    loop {
        tokio::select! {
            _ = shutdown_rx.next() => {
                break;
            }
            _ = update_timer.tick() => {
                update_status(&bot.client, &mut prev_rst, &rst).await;
            }
            ev = r.recv() => {
                let ev = match ev {
                    Ok(ev) => ev,
                    Err(_) => break,
                };

                debug!("{:?}", ev);

                match ev {
                    mumble::Event::Message(ev) => commands::handle_message_event(&mut bot, &ev).await,
                    _ => {}
                }
            }
            ev = room_events.recv() => {
                let ev = match ev {
                    Ok(ev) => ev,
                    Err(_) => break,
                };

                debug!("{:?}", ev);

                match ev {
                    RoomEvent::PlayerEvent(p) => {
                        match p {
                            PlayerEvent::Playing { now, pos } => {
                                rst.playing_since = Some(now);
                                rst.position = pos;
                                update_status(&bot.client, &mut prev_rst, &rst).await;
                            },
                            PlayerEvent::Paused { pos, .. } => {
                                rst.playing_since = None;
                                rst.position = pos;
                                update_status(&bot.client, &mut prev_rst, &rst).await;
                            },
                        }
                    }
                    RoomEvent::TrackChanged(t, len) => {
                        rst.title = t.object().title().unwrap_or("Unnamed Track").to_string();
                        rst.total_duration = len;
                        rst.position = Duration::ZERO;
                        update_status(&bot.client, &mut prev_rst, &rst).await;
                    }
                    RoomEvent::TrackCleared => {
                        rst.title = "(none)".to_string();
                        rst.total_duration = Duration::ZERO;
                        rst.position = Duration::ZERO;
                        update_status(&bot.client, &mut prev_rst, &rst).await;
                    }
                }
            }
        }
    }

    let _ = bot.client.message_my_channel("quitting!").await;
    bot.client.close().await.unwrap();
}

pub struct Bot {
    client: MumbleClient,
    room: Room,
    shutdown_fuse: Option<oneshot::Sender<()>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RoomStatus {
    title: String,
    album_title: String,
    artist: String,
    position: Duration,
    playing_since: Option<Instant>,
    total_duration: Duration,
}

impl RoomStatus {
    pub fn should_update(&self, other: &RoomStatus) -> bool {
        self.playing_since.is_some() || self != other
    }
}

impl Default for RoomStatus {
    fn default() -> Self {
        RoomStatus {
            title: "(none)".to_string(),
            album_title: "(none)".to_string(),
            artist: "(none)".to_string(),
            position: Default::default(),
            playing_since: None,
            total_duration: Default::default(),
        }
    }
}

async fn update_status(client: &MumbleClient, prev_st: &mut RoomStatus, st: &RoomStatus) {
    if !st.should_update(&prev_st) {
        *prev_st = st.clone();
        return;
    }

    let state_ch = match st.playing_since {
        None => "⏸︎",
        Some(_) => "⏵︎",
    };

    let current_position = match st.playing_since {
        None => st.position,
        Some(then) => {
            let diff = Instant::now().duration_since(then);
            min(st.position + diff, st.total_duration)
        }
    };

    let str = format!(
        "{}<br>{}<br>{}<br>[{}] [{} / {}]<hr>{} {}",
        st.title,
        st.album_title,
        st.artist,
        state_ch,
        FmtDuration(current_position),
        FmtDuration(st.total_duration),
        CRATE_NAME,
        CRATE_VERSION,
    );

    client.set_comment(str).await.unwrap();

    *prev_st = st.clone();
}

struct FmtDuration(Duration);

impl Display for FmtDuration {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let a = self.0.as_secs();
        let secs = a % 60;
        let a = a / 60;
        let mins = a % 60;
        let hours = a / 60;
        write!(f, "{:02}:{:02}:{:02}", hours, mins, secs)
    }
}

pub struct LaunchConfig {
    pub data_dir: PathBuf,
    pub db_url: String,
    pub db_pool_size: u32,
    pub db_pool_size_min: u32,

    // temporary
    pub mumble_domain: String,
    pub mumble_port: u16,
    pub mumble_cert: Option<String>,
    pub name: String,
}

fn load_config() -> LaunchConfig {
    use cmdparser::CommandDispatcher;
    use cmdparser::ExecSource;
    use cmdparser::SimpleExecutor;

    let mut data_dir = None;
    let mut db_url = None;
    let mut db_pool_size = None;
    let mut db_pool_size_min = None;
    let mut mumble = None;
    let mut mumble_cert = None;
    let mut name = None;

    let mut cd = CommandDispatcher::new(SimpleExecutor::new(|cmd, args| match cmd {
        "data_dir" => data_dir = Some(args[0].to_string()),
        "db_url" => db_url = Some(args[0].to_string()),
        "db_pool_size" => {
            db_pool_size = Some(
                args[0]
                    .parse::<u32>()
                    .expect("db_pool_size must be a positive integer"),
            )
        }
        "db_pool_size_scale" => {
            db_pool_size = Some(
                args[0]
                    .parse::<u32>()
                    .expect("db_pool_size must be a positive integer")
                    * num_cpus::get() as u32,
            )
        }
        "db_pool_size_min" => {
            db_pool_size_min = Some(
                args[0]
                    .parse::<u32>()
                    .expect("db_pool_size_min must be a positive integer"),
            )
        }
        "db_pool_size_min_scale" => {
            db_pool_size_min = Some(
                args[0]
                    .parse::<u32>()
                    .expect("db_pool_size_min must be a positive integer")
                    * num_cpus::get() as u32,
            )
        }
        "mumble" => {
            mumble = Some((
                args[0].to_string(),
                args[1]
                    .parse::<u16>()
                    .expect("mumble second param must be port"),
            ))
        }
        "mumble_cert" => mumble_cert = Some(args[0].to_string()),
        "name" => name = Some(args[0].to_string()),
        _ => eprintln!("Ignoring invalid bootstrap command '{}'!", cmd),
    }));
    cd.scheduler()
        .exec_path("srvrc", ExecSource::Event)
        .expect("Failed to load srvrc");
    cd.resume_until_empty();

    let db_pool_size = db_pool_size.unwrap_or_else(|| num_cpus::get() as u32);
    let (mumble_domain, mumble_port) = mumble.expect("mumble connection not set!");

    LaunchConfig {
        data_dir: data_dir.expect("data_dir not set!").into(),
        db_url: db_url.expect("db_url not set!"),
        db_pool_size,
        db_pool_size_min: db_pool_size_min.unwrap_or(db_pool_size),
        mumble_domain,
        mumble_port,
        mumble_cert,
        name: name.unwrap_or_else(|| "r2dj".to_string()),
    }
}
