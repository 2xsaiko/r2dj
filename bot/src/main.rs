use std::borrow::Cow;
use std::fmt;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::time::Duration;

use log::{info, LevelFilter};
use simplelog::{Config, TerminalMode};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

use mumble::{Event, MumbleConfig};
use player2x::ffplayer::Player;
use uuid::Uuid;
use std::str::FromStr;

const CRATE_NAME: &str = env!("CARGO_PKG_NAME");
const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

mod config;
mod player;
mod spotify;
mod db;

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

    let pool = PgPoolOptions::new()
        .max_connections(config.db_pool_size)
        .min_connections(config.db_pool_size_min)
        .idle_timeout(Some(Duration::from_secs(600)))
        .connect(&config.db_url).await.unwrap();

    let id = Uuid::from_str("99b071f7-bdae-48b4-9c0a-aac91332c348").unwrap();
    let pl = player::Playlist::load(id, &pool).await.unwrap();

    println!("{:#?}", pl);

    // let config = MumbleConfig {
    //     username: config.name.clone(),
    // };
    //
    // let client = mumble::MumbleClient::connect(host, port, config)
    //     .await
    //     .unwrap();
    // let st = client.server_state();
    //
    // let mut r = client.event_subscriber();
    //
    // let mut player = Player::new("04 - Bone Dry.mp3", client.audio_input()).unwrap();
    // player.play().await;
    //
    // while let Ok(ev) = r.recv().await {
    //     match ev {
    //         Event::Message { actor, message, .. } => {
    //             let st = st.lock().await;
    //
    //             let name: Cow<_> = match actor {
    //                 None => "<unknown>".into(),
    //                 Some(r) => r.get(&st).unwrap().name().to_string().into(),
    //             };
    //
    //             if message == "stop!" {
    //                 break;
    //             }
    //
    //             println!("{}: {}", name, message);
    //
    //             drop(st);
    //
    //             if actor != Some(client.user()) {
    //                 client
    //                     .send_channel_message(&format!("{}: {}", name, message))
    //                     .await;
    //             }
    //         }
    //         _ => {}
    //     }
    // }
    //
    // client.send_channel_message("quitting!").await;
    //
    // client.close().await;
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
    pub name: String,
}

fn load_config() -> LaunchConfig {
    use cmdparser::CommandDispatcher;
    use cmdparser::SimpleExecutor;
    use cmdparser::ExecSource;

    let mut data_dir = None;
    let mut db_url = None;
    let mut db_pool_size = None;
    let mut db_pool_size_min = None;
    let mut mumble = None;
    let mut name = None;

    let mut cd = CommandDispatcher::new(SimpleExecutor::new(|cmd, args| match cmd {
        "data_dir" => data_dir = Some(args[0].to_string()),
        "db_url" => db_url = Some(args[0].to_string()),
        "db_pool_size" => db_pool_size = Some(args[0].parse::<u32>().expect("db_pool_size must be a positive integer")),
        "db_pool_size_scale" => db_pool_size = Some(args[0].parse::<u32>().expect("db_pool_size must be a positive integer") * num_cpus::get() as u32),
        "db_pool_size_min" => db_pool_size_min = Some(args[0].parse::<u32>().expect("db_pool_size_min must be a positive integer")),
        "db_pool_size_min_scale" => db_pool_size_min = Some(args[0].parse::<u32>().expect("db_pool_size_min must be a positive integer") * num_cpus::get() as u32),
        "mumble" => mumble = Some((args[0].to_string(), args[1].parse::<u16>().expect("mumble second param must be port"))),
        "name" => name = Some(args[0].to_string()),
        _ => eprintln!("Ignoring invalid bootstrap command '{}'!", cmd),
    }));
    cd.scheduler().exec_path("srvrc", ExecSource::Event).expect("Failed to load srvrc");
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
        name: name.unwrap_or_else(|| "r2dj".to_string()),
    }
}