use sqlx::PgConnection;
use url::Url;
use youtube_dl::{SingleVideo, YoutubeDlOutput};

use crate::entity::import::ImportError;

use super::{Source, Track};

impl Track {
    pub async fn load_by_youtube_id(id: &str, db: &mut PgConnection) -> sqlx::Result<Self> {
        // language=SQL
        let r = sqlx::query!("SELECT track FROM track_provider WHERE youtube_id = $1", id)
            .fetch_one(&mut *db)
            .await?
            .track;
        Track::load(r, &mut *db).await
    }

    pub async fn import_by_youtube_id(
        id: &str,
        db: &mut PgConnection,
    ) -> Result<Self, ImportError> {
        match Track::load_by_youtube_id(&id, db).await {
            Ok(v) => return Ok(v),
            Err(sqlx::Error::RowNotFound) => {}
            Err(e) => return Err(e.into()),
        };

        let url = Url::parse_with_params("https://www.youtube.com/watch", [("v", id)])?;

        let output = youtube_dl::YoutubeDl::new(url.into_string()).run()?;

        let output = match output {
            YoutubeDlOutput::Playlist(_) => unreachable!(),
            YoutubeDlOutput::SingleVideo(v) => v,
        };

        let track = Track::import_from_youtube(&output, None).await?;
        Ok(track)
    }

    pub async fn import_from_youtube(
        metadata: &SingleVideo,
        db: Option<&mut PgConnection>,
    ) -> sqlx::Result<Self> {
        if let Some(db) = db {
            match Track::load_by_youtube_id(&metadata.id, db).await {
                Ok(v) => return Ok(v),
                Err(sqlx::Error::RowNotFound) => {}
                Err(e) => return Err(e),
            };
        }

        let mut track = Track::new();
        track.set_title(Some(metadata.title.clone()));
        track.add_provider(Source::Youtube(metadata.id.clone()));
        Ok(track)
    }
}
