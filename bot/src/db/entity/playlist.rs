use futures::future::BoxFuture;
use futures::{FutureExt, StreamExt};
use sqlx::postgres::PgQueryResult;
use sqlx::{PgConnection};
use uuid::Uuid;

use crate::db::{entity, object, objgen};
use crate::player::treepath::TreePath;

mod import;

#[derive(Debug, Clone)]
pub struct Playlist {
    object: object::Playlist,
    entries: Vec<PlaylistEntry>,
}

impl Playlist {
    pub fn new() -> Self {
        let mut pl = Playlist {
            object: object::Playlist::new(),
            entries: Vec::new(),
        };
        pl.set_title("Playlist");
        pl
    }

    pub async fn load(id: Uuid, db: &mut PgConnection) -> sqlx::Result<Self> {
        let object = object::Playlist::load(id, db).await?;
        Playlist::load_from(object, db).await
    }

    fn load_from(object: object::Playlist, db: &mut PgConnection) -> BoxFuture<sqlx::Result<Self>> {
        async move {
            let mut playlist = Playlist::new();
            playlist.object = object;
            playlist.load_more(db).await?;
            Ok(playlist)
        }
        .boxed()
    }
}

impl Playlist {
    pub fn set_title(&mut self, title: impl Into<String>) {
        self.object.set_title(title);
    }

    pub fn set_spotify_id(&mut self, id: Option<impl Into<String>>) {
        self.object.set_spotify_id(id);
    }

    pub fn set_youtube_id(&mut self, id: Option<impl Into<String>>) {
        self.object.set_youtube_id(id);
    }

    pub fn push_track(&mut self, track: entity::Track) {
        self.entries.push(PlaylistEntry {
            id: Uuid::new_v4(),
            content: Content::Track(track),
        });
    }

    pub fn push_playlist(&mut self, playlist: Playlist) {
        self.entries.push(PlaylistEntry {
            id: Uuid::new_v4(),
            content: Content::Playlist(playlist),
        });
    }

    pub fn push_content(&mut self, content: Content) {
        self.entries.push(PlaylistEntry {
            id: Uuid::new_v4(),
            content,
        })
    }

    pub fn add_track(
        &mut self,
        track: entity::Track,
        path: impl AsRef<TreePath>,
    ) -> Result<(), entity::Track> {
        match self.add_content(Content::Track(track), path) {
            Ok(_) => Ok(()),
            Err(Content::Track(v)) => Err(v),
            _ => unreachable!(),
        }
    }

    pub fn add_playlist(
        &mut self,
        playlist: Playlist,
        path: impl AsRef<TreePath>,
    ) -> Result<(), Playlist> {
        match self.add_content(Content::Playlist(playlist), path) {
            Ok(_) => Ok(()),
            Err(Content::Playlist(v)) => Err(v),
            _ => unreachable!(),
        }
    }

    pub fn add_content(
        &mut self,
        content: Content,
        path: impl AsRef<TreePath>,
    ) -> Result<(), Content> {
        let path = path.as_ref();

        if path.is_empty() {
            self.push_content(content);
            Ok(())
        } else {
            let idx = path.to_slice()[0];
            let el = match self.entries.get_mut(idx as usize) {
                None => return Err(content),
                Some(v) => v,
            };

            match &mut el.content {
                Content::Track(_) => Err(content),
                Content::Playlist(pl) => pl.add_content(content, &path[1..]),
            }
        }
    }

    pub fn entries(&self) -> &[PlaylistEntry] {
        &self.entries
    }

    pub fn get_entry(&self, path: impl AsRef<TreePath>) -> Option<&Content> {
        let path = path.as_ref();

        if path.is_empty() {
            None // check this yourself
        } else {
            let idx = path.to_slice()[0];
            let el = self.entries.get(idx as usize)?;

            if path.len() == 1 {
                Some(&el.content)
            } else {
                match &el.content {
                    Content::Track(_) => None,
                    Content::Playlist(pl) => pl.get_entry(&path[1..]),
                }
            }
        }
    }

    pub fn get_playlist(&self, path: impl AsRef<TreePath>) -> Option<&Playlist> {
        let path = path.as_ref();

        if path.is_empty() {
            Some(self)
        } else {
            match self.get_entry(path) {
                Some(Content::Playlist(pl)) => Some(pl),
                _ => None,
            }
        }
    }

    pub fn get_track(&self, path: impl AsRef<TreePath>) -> Option<&entity::Track> {
        match self.get_entry(path) {
            Some(Content::Track(t)) => Some(t),
            _ => None,
        }
    }
}

impl Playlist {
    pub async fn reload(&mut self, db: &mut PgConnection) -> sqlx::Result<()> {
        if let Some(id) = self.object.id() {
            self.object = object::Playlist::load(id, db).await?;
            self.load_more(db).await?;
        }

        Ok(())
    }

    async fn load_more(&mut self, db: &mut PgConnection) -> sqlx::Result<()> {
        let id = self.object.id().expect("No valid object loaded");

        self.entries.clear();
        // language=SQL
        let rows = sqlx::query!(
            "SELECT id, track, sub_playlist
                 FROM playlist_entry
                 WHERE playlist = $1
                 ORDER BY index",
            id
        )
        .fetch(&mut *db)
        .collect::<Vec<_>>()
        .await;

        for row in rows {
            let row = row?;

            let content = if let Some(track_id) = row.track {
                let track = entity::Track::load(track_id, &mut *db).await?;
                Content::Track(track)
            } else if let Some(sub_playlist_id) = row.sub_playlist {
                let sub_playlist = Playlist::load(sub_playlist_id, &mut *db).await?;
                Content::Playlist(sub_playlist)
            } else {
                unimplemented!()
            };

            self.entries.push(PlaylistEntry {
                id: row.id,
                content,
            });
        }

        Ok(())
    }

    pub fn save<'a>(
        &'a mut self,
        db: &'a mut PgConnection,
    ) -> BoxFuture<'a, objgen::Result<PgQueryResult>> {
        async move {
            let mut r = self.object.save(db).await?;
            let id = self.object.id().unwrap();

            // for now, remove everything and re-insert for simplicity
            // might add some more intelligent update mechanism later if this
            // becomes too slow

            // language=SQL
            r.extend([
                sqlx::query!("DELETE FROM playlist_entry WHERE playlist = $1", id)
                    .execute(&mut *db)
                    .await?,
            ]);

            for (idx, entry) in self.entries.iter_mut().enumerate() {
                // language=SQL
                match &mut entry.content {
                    Content::Track(track) => {
                        r.extend([track.save(db).await?]);

                        r.extend([sqlx::query!(
                            "INSERT INTO playlist_entry (id, playlist, index, track) VALUES ($1, $2, $3, $4)",
                            entry.id,
                            id,
                            idx as u32,
                            track.object().id().unwrap()
                        )
                        .execute(&mut *db)
                        .await?]);
                    }
                    Content::Playlist(playlist) => {
                        r.extend([playlist.save(db).await?]);

                        r.extend([sqlx::query!(
                            "INSERT INTO playlist_entry (id, playlist, index, track) VALUES ($1, $2, $3, $4)",
                            entry.id,
                            id,
                            idx as u32,
                            playlist.object().id().unwrap()
                        )
                        .execute(&mut *db)
                        .await?]);
                    }
                }
            }

            Ok(r)
        }.boxed()
    }

    pub fn object(&self) -> &object::Playlist {
        &self.object
    }
}

#[derive(Debug, Clone)]
pub struct PlaylistEntry {
    id: Uuid,
    content: Content,
}

impl PlaylistEntry {
    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn content(&self) -> &Content {
        &self.content
    }
}

#[derive(Debug, Clone)]
pub enum Content {
    Track(entity::Track),
    Playlist(Playlist),
}
