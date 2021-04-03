use std::time::Duration;

use log::{info, LevelFilter};
use simplelog::{Config, TerminalMode};

use crate::ffmpeg::{FfmpegConfig, Format, PathSource, PipeDest};
use crate::mumble::MumbleConfig;
use crate::mixer::new_mixer;
use crate::ffplayer::Player;
use std::fmt::{Display, Formatter};
use std::fmt;
use tokio::io::{BufReader, AsyncBufReadExt};

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

    // let (input, output) = new_buffer(2 * 2 * 480);
    let (input, output) = new_mixer();
    let input2 = input.clone();

    // tokio::spawn(ffmpeg::ffpipe(
    //     PathSource::new("04 - Bone Dry.mp3"),
    //     PipeDest::new(input),
    //     FfmpegConfig::default()
    //         .output_format(Format::native_pcm(48000))
    //         .start_at(Duration::from_secs(60)),
    // ));
    //
    // tokio::spawn(ffmpeg::ffpipe(
    //     PathSource::new("YouTube Kacke - Der Weihnachtsmann.mkv"),
    //     PipeDest::new(input2),
    //     FfmpegConfig::default()
    //         .output_format(Format::native_pcm(48000))
    //         .start_at(Duration::from_secs(60)),
    // ));

    tokio::spawn(async move {
        client.consume(output).await.unwrap();
        client.close().await;
    });

    let mut player = Player::new("04 - Bone Dry.mp3", input).unwrap();

    let mut stdin = BufReader::new(tokio::io::stdin());
    let mut buf = String::new();

    loop {
        println!("{} / {}", FmtDuration(player.length()), FmtDuration(player.position().await));
        buf.clear();
        stdin.read_line(&mut buf).await.unwrap();
        player.play().await;
        println!("{} / {}", FmtDuration(player.length()), FmtDuration(player.position().await));
        buf.clear();
        stdin.read_line(&mut buf).await.unwrap();
        player.pause().await;
    }
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