use std::fmt;
use std::fmt::{Display, Formatter};
use std::time::Duration;

use log::{info, LevelFilter};
use simplelog::{Config, TerminalMode};
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::ffplayer::{Player, PlayerEvent};
use crate::mixer::new_mixer;
use crate::mumble::{MumbleConfig, Event};
use std::borrow::Cow;

const CRATE_NAME: &str = env!("CARGO_PKG_NAME");
const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

mod buffer;
mod config;
mod ffmpeg;
mod ffplayer;
mod ffprobe;
mod mixer;
mod mumble;
mod player;
mod spotify;
mod util;

#[tokio::main]
async fn main() {
    let host = "dblsaiko.net";
    let port = 64738;

    simplelog::TermLogger::init(
        LevelFilter::Debug,
        Config::default(),
        TerminalMode::default(),
    )
    .unwrap();

    info!("Starting {} {}", CRATE_NAME, CRATE_VERSION);

    let config = MumbleConfig {
        username: "r2dj".to_string(),
    };

    let client = mumble::MumbleClient::connect(host, port, config)
        .await
        .unwrap();
    let st = client.server_state();

    let (input, output) = new_mixer();

    let mut r = client.event_listener();

    // tokio::spawn(async move {
    //     client.consume(output).await.unwrap();
    //     client.close().await;
    // });

    while let Ok(ev) = r.recv().await {
        match ev {
            Event::Message { actor, message, .. } => {
                let st = st.lock().await;

                let name: Cow<_> = match actor {
                    None => "<unknown>".into(),
                    Some(r) => r.get(&st).unwrap().name().to_string().into(),
                };

                println!("{}: {}", name, message);

                drop(st);

                if actor != Some(client.user()) {
                    client.send_channel_message(&format!("{}: {}", name, message)).await;
                }
            }
            _ => {}
        }
    }

    client.close().await;

    // let mut player = Player::new("04 - Bone Dry.mp3", input).unwrap();
    // player.seek(Duration::from_secs(60 * 4 + 30)).await;
    //
    // let mut stdin = BufReader::new(tokio::io::stdin());
    // let mut buf = String::new();
    //
    // loop {
    //     println!(
    //         "{} / {}",
    //         FmtDuration(player.length()),
    //         FmtDuration(player.position().await)
    //     );
    //     buf.clear();
    //     stdin.read_line(&mut buf).await.unwrap();
    //     player.play().await;
    //     println!(
    //         "{} / {}",
    //         FmtDuration(player.length()),
    //         FmtDuration(player.position().await)
    //     );
    //     buf.clear();
    //     let mut l = player.event_listener();
    //     loop {
    //         tokio::select! {
    //             _ = stdin.read_line(&mut buf) => {
    //                 player.pause().await;
    //                 break;
    //             },
    //             msg = l.recv() => {
    //                 println!("{:?}", msg);
    //                 let msg = msg.unwrap();
    //                 match msg {
    //                     PlayerEvent::Playing { .. } => {}
    //                     PlayerEvent::Paused { stopped, .. } => {
    //                         if stopped {
    //                             player.seek(Duration::from_secs(0)).await;
    //                         }
    //                         break;
    //                     }
    //                 }
    //             },
    //         }
    //     }
    // }
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
