use std::borrow::Cow;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;

use crate::db::entity::track::{Source, TrackProvider};
use thiserror::Error;
use tokio::process::Command;
use url::Url;
use uuid::Uuid;

impl TrackProvider {
    pub async fn media_path(&self) -> Result<Cow<'_, Path>, GetFileError> {
        match &self.source() {
            Source::Local(pb) => Ok(pb.into()),
            Source::Url(url) => media_path_url(&self.id(), url).await.map(|v| v.into()),
            Source::Spotify(id) => {
                todo!()
            }
            Source::Youtube(id) => media_path_url(
                &self.id(),
                &Url::parse(&format!("https://www.youtube.com/watch?v={}", id)).unwrap(),
            )
            .await
            .map(|v| v.into()),
        }
    }
}

async fn media_path_url(id: &Uuid, url: &Url) -> Result<PathBuf, GetFileError> {
    let mut path = PathBuf::from("media/cached");
    let mut buffer = Uuid::encode_buffer();
    let id = id.to_simple_ref().encode_upper(&mut buffer);
    path.push(&id[..2]);
    path.push(&id);
    path.set_extension("flac");

    if !path.is_file() {
        youtube_dl(url, &path).await?;
    }

    Ok(path.into())
}

#[derive(Debug, Error)]
pub enum GetFileError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("youtube-dl error {0}")]
    ExitStatus(ExitStatus),
}

async fn youtube_dl<P>(url: &Url, output: P) -> Result<(), GetFileError>
where
    P: AsRef<Path>,
{
    // FIXME this isn't converting the audio to flac...
    let mut cmd = Command::new("youtube-dl");
    cmd.arg("-x").arg("--audio-format").arg("flac");
    cmd.arg("-o").arg(output.as_ref()).arg(url.as_str());
    match cmd.status().await {
        Ok(st) if st.success() => Ok(()),
        Ok(st) => Err(GetFileError::ExitStatus(st)),
        Err(e) => Err(e.into()),
    }
}
