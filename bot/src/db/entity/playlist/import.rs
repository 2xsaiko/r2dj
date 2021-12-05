use sqlx::PgConnection;
use url::Url;
use youtube_dl::YoutubeDlOutput;

use crate::db::object;
use crate::entity::import::ImportError;
use crate::entity::Track;

use super::Playlist;

impl Playlist {
    pub async fn load_by_youtube_id(id: &str, db: &mut PgConnection) -> sqlx::Result<Self> {
        let object = object::Playlist::load_by_youtube_id(id, db).await?;
        Playlist::load_from(object, db).await
    }

    pub async fn import_by_youtube_id(
        id: &str,
        db: &mut PgConnection,
    ) -> Result<Self, ImportError> {
        match Playlist::load_by_youtube_id(&id, db).await {
            Ok(v) => return Ok(v),
            Err(sqlx::Error::RowNotFound) => {}
            Err(e) => return Err(e.into()),
        };

        let mut pl = Playlist::new();
        pl.set_youtube_id(Some(id.to_string()));
        pl.update_from_youtube(true, db).await?;

        Ok(pl)
    }

    pub async fn update_content_from_youtube(&mut self, db: &mut PgConnection) -> Result<(), ImportError> {
        self.update_from_youtube(false, db).await
    }

    async fn update_from_youtube(&mut self, initial_setup: bool, db: &mut PgConnection) -> Result<(), ImportError> {
        let id = match self.object().youtube_id() {
            None => return Ok(()),
            Some(v) => v,
        };

        let url = Url::parse_with_params("https://www.youtube.com/playlist", [("list", id)])?;

        let output = youtube_dl::YoutubeDl::new(url.into_string())
            .flat_playlist(true)
            .run()?;

        let output = match output {
            YoutubeDlOutput::SingleVideo(_) => unreachable!(),
            YoutubeDlOutput::Playlist(v) => v,
        };

        if initial_setup {
            if let Some(title) = output.title {
                self.set_title(title);
            }
        }

        self.entries.clear();

        for el in output.entries.iter().flatten() {
            let track = Track::import_from_youtube(el, Some(db)).await?;
            self.push_track(track);
        }

        Ok(())
    }
}
