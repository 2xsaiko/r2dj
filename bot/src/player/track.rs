use std::borrow::Cow;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;

use futures::TryStreamExt;
use sqlx::PgPool;
use thiserror::Error;
use tokio::process::Command;
use url::Url;
use uuid::Uuid;

use crate::db::types::TrackProviderType;

#[derive(Debug, Clone)]
pub struct Track {
    id: Uuid,
    title: Option<String>,
    providers: Vec<TrackProvider>,
}

#[derive(Debug, Clone)]
pub struct TrackProvider {
    id: Uuid,
    data: TrackProviderData,
}

#[derive(Debug, Clone)]
pub enum TrackProviderData {
    Local(PathBuf),
    Url(Url),
    // Spotify(???),
    Youtube(String),
}

impl Track {
    pub fn new() -> Self {
        Track {
            id: Uuid::new_v4(),
            title: None,
            providers: vec![],
        }
    }

    pub async fn load(id: Uuid, db: &PgPool) -> sqlx::Result<Self> {
        let track = sqlx::query!("SELECT id, title FROM track WHERE id = $1", id)
            .fetch_one(db)
            .await?;

        let providers: Vec<_> = sqlx::query!(
            r#"SELECT id, type as "type: TrackProviderType", source FROM track_provider WHERE track = $1"#,
            id
        )
        .fetch(db)
        .try_collect()
        .await?;

        let providers = providers
            .into_iter()
            .map(|el| {
                let data = match el.r#type {
                    TrackProviderType::Local => TrackProviderData::Local(el.source.into()),
                    TrackProviderType::Url => TrackProviderData::Url(
                        Url::parse(&el.source).expect("non-URL data in track_provider.source"),
                    ),
                    TrackProviderType::Spotify => unimplemented!(),
                    TrackProviderType::Youtube => TrackProviderData::Youtube(el.source),
                };

                TrackProvider { id: el.id, data }
            })
            .collect();

        Ok(Track {
            id: track.id,
            title: track.title,
            providers,
        })
    }

    pub fn providers(&self) -> &[TrackProvider] {
        &self.providers
    }
}

impl TrackProvider {
    pub async fn media_path(&self) -> Result<Cow<'_, Path>, GetFileError> {
        match &self.data {
            TrackProviderData::Local(pb) => Ok(pb.into()),
            TrackProviderData::Url(url) => media_path_url(&self.id, url).await.map(|v| v.into()),
            TrackProviderData::Youtube(id) => media_path_url(
                &self.id,
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
    let mut cmd = Command::new("youtube-dl");
    cmd.arg("-x").arg("--audio-format").arg("flac");
    cmd.arg("-o").arg(output.as_ref()).arg(url.as_str());
    match cmd.status().await {
        Ok(st) if st.success() => Ok(()),
        Ok(st) => Err(GetFileError::ExitStatus(st)),
        Err(e) => Err(e.into()),
    }
}
