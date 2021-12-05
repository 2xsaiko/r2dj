pub use playlist::Playlist;
pub use track::Track;

pub mod playlist;
pub mod track;

pub mod import {
    use thiserror::Error;

    #[derive(Debug, Error)]
    pub enum ImportError {
        #[error("failed to parse video URL: {0}")]
        UrlParseError(#[from] url::ParseError),
        #[error("{0}")]
        Sqlx(#[from] sqlx::Error),
        #[error("youtube-dl error: {0}")]
        YoutubeDl(#[from] youtube_dl::Error),
    }
}