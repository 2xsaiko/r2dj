use std::path::PathBuf;

use futures::StreamExt;
use sqlx::postgres::PgQueryResult;
use sqlx::{Acquire, PgConnection};
use url::Url;
use uuid::Uuid;

use crate::db::{object, objgen};

mod import;

#[derive(Debug, Clone)]
pub struct Track {
    object: object::Track,
    providers: Vec<TrackProvider>,
}

#[derive(Debug, Clone)]
pub struct TrackProvider {
    id: Uuid,
    source: Source,
}

impl TrackProvider {
    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn source(&self) -> &Source {
        &self.source
    }
}

#[derive(Debug, Clone)]
pub enum Source {
    Local(PathBuf),
    Url(Url),
    Spotify(String),
    Youtube(String),
}

impl Track {
    pub fn new() -> Self {
        Track {
            object: object::Track::new(),
            providers: Vec::new(),
        }
    }

    pub async fn load(id: Uuid, db: &mut PgConnection) -> sqlx::Result<Self> {
        let mut track = Track::new();
        track.object = object::Track::load(id, db).await?;
        track.load_more(db).await?;
        Ok(track)
    }

    pub fn set_title(&mut self, title: Option<String>) {
        self.object.set_title(title);
    }

    pub fn title(&self) -> Option<&str> {
        self.object.title()
    }

    pub fn add_provider(&mut self, source: Source) {
        let id = Uuid::new_v4();
        self.providers.push(TrackProvider { id, source });
    }

    pub fn providers(&self) -> &[TrackProvider] {
        &self.providers
    }
}

impl Track {
    pub async fn reload(&mut self, db: &mut PgConnection) -> sqlx::Result<()> {
        if let Some(id) = self.object.id() {
            self.object = object::Track::load(id, db).await?;
            self.load_more(db).await?;
        }

        Ok(())
    }

    async fn load_more(&mut self, db: &mut PgConnection) -> sqlx::Result<()> {
        let id = self.object.id().expect("No valid object loaded");

        self.providers.clear();
        // language=SQL
        let mut rows = sqlx::query!(
            "SELECT id, local_path, url, spotify_id, youtube_id
             FROM track_provider
             WHERE track = $1",
            id
        )
        .fetch(&mut *db);

        while let Some(row) = rows.next().await {
            let row = row?;

            let source = if let Some(local_path) = row.local_path {
                Source::Local(local_path.into())
            } else if let Some(url) = row.url {
                Source::Url(url.parse().expect("invalid URL in track_provider.url"))
            } else if let Some(spotify_id) = row.spotify_id {
                Source::Spotify(spotify_id)
            } else if let Some(youtube_id) = row.youtube_id {
                Source::Youtube(youtube_id)
            } else {
                unimplemented!()
            };

            self.providers.push(TrackProvider { id: row.id, source });
        }

        Ok(())
    }

    pub async fn save(&mut self, db: &mut PgConnection) -> objgen::Result<PgQueryResult> {
        let mut r = self.object.save(db).await?;

        // language=SQL
        r.extend([sqlx::query!(
            "DELETE FROM track_provider WHERE track = $1",
            self.object.id()
        )
        .execute(&mut *db)
        .await?]);

        for p in self.providers.iter() {
            let (local_path, url, spotify_id, youtube_id) = match &p.source {
                Source::Local(v) => (Some(v.to_str().unwrap()), None, None, None),
                Source::Url(v) => (None, Some(v.as_str()), None, None),
                Source::Spotify(v) => (None, None, Some(v), None),
                Source::Youtube(v) => (None, None, None, Some(v)),
            };

            // language=SQL
            r.extend([sqlx::query!("INSERT INTO track_provider (id, track, local_path, url, spotify_id, youtube_id) VALUES ($1, $2, $3, $4, $5, $6)", p.id, self.object.id(), local_path, url, spotify_id, youtube_id).execute(&mut *db).await?]);
        }

        Ok(r)
    }

    pub fn object(&self) -> &object::Track {
        &self.object
    }
}
