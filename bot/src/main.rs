use std::time::Duration;

use log::{info, LevelFilter};
use simplelog::{Config, TerminalMode};

use crate::buffer::new_buffer;
use crate::ffmpeg::{FfmpegConfig, Format, PathSource, PipeDest};
use crate::mumble::MumbleConfig;
use crate::mixer::new_mixer;

const CRATE_NAME: &str = env!("CARGO_PKG_NAME");
const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

mod buffer;
mod config;
mod ffmpeg;
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

    tokio::spawn(ffmpeg::ffpipe(
        PathSource::new("04 - Bone Dry.mp3"),
        PipeDest::new(input),
        FfmpegConfig::default()
            .output_format(Format::native_pcm(48000))
            .start_at(Duration::from_secs(60)),
    ));

    tokio::spawn(ffmpeg::ffpipe(
        PathSource::new("YouTube Kacke - Der Weihnachtsmann.mkv"),
        PipeDest::new(input2),
        FfmpegConfig::default()
            .output_format(Format::native_pcm(48000))
            .start_at(Duration::from_secs(60)),
    ));

    client.consume(output).await.unwrap();

    client.close().await;
}
