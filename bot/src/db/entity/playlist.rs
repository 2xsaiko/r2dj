use futures::StreamExt;
use sqlx::postgres::PgQueryResult;
use sqlx::{Acquire, PgConnection};
use uuid::Uuid;

use crate::db::{object, objgen};

#[derive(Debug, Clone)]
pub struct Playlist {
    object: object::Playlist,
    entries: Vec<PlaylistEntry>,
}

impl Playlist {
    pub fn new() -> Self {
        Playlist {
            object: object::Playlist::new(),
            entries: Vec::new(),
        }
    }

    pub async fn load(id: Uuid, db: &mut PgConnection) -> sqlx::Result<Self> {
        let mut playlist = Playlist::new();
        playlist.object = object::Playlist::load(id, db).await?;
        playlist.load_more(db).await?;
        Ok(playlist)
    }
}

impl Playlist {
    async fn reload(&mut self, db: &mut PgConnection) -> sqlx::Result<()> {
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
        let mut rows = sqlx::query!(
            "SELECT id, track, sub_playlist
                 FROM playlist_entry
                 WHERE playlist = $1
                 ORDER BY index",
            id
        )
        .fetch(db);

        while let Some(row) = rows.next().await {
            let row = row?;

            let content = if let Some(track) = row.track {
                Content::Track(track)
            } else if let Some(sub_playlist) = row.sub_playlist {
                Content::Playlist(sub_playlist)
            } else {
                unreachable!()
            };

            self.entries.push(PlaylistEntry {
                id: row.id,
                content,
            });
        }

        Ok(())
    }

    async fn save(&mut self, db: &mut PgConnection) -> objgen::Result<PgQueryResult> {
        let mut ta = db.begin().await?;
        let mut r = self.object.save(&mut ta).await?;
        let id = self.object.id().unwrap();

        // for now, remove everything and re-insert for simplicity
        // might add some more intelligent update mechanism later if this
        // becomes too slow

        // language=SQL
        r.extend([
            sqlx::query!("DELETE FROM playlist_entry WHERE playlist = $1", id)
                .execute(&mut ta)
                .await?,
        ]);

        for (idx, entry) in self.entries.iter().enumerate() {
            // language=SQL
            match entry.content {
                Content::Track(track) => {
                    r.extend([sqlx::query!(
                        "INSERT INTO playlist_entry (id, playlist, index, track) VALUES ($1, $2, $3, $4)",
                        entry.id,
                        id,
                        idx as u32,
                        track
                    )
                    .execute(&mut ta)
                    .await?]);
                }
                Content::Playlist(playlist) => {
                    r.extend([sqlx::query!(
                        "INSERT INTO playlist_entry (id, playlist, index, track) VALUES ($1, $2, $3, $4)",
                        entry.id,
                        id,
                        idx as u32,
                        playlist
                    )
                    .execute(&mut ta)
                    .await?]);
                }
            }
        }

        ta.commit().await?;
        Ok(r)
    }

    fn object(&self) -> &object::Playlist {
        &self.object
    }
}

#[derive(Debug, Clone)]
struct PlaylistEntry {
    id: Uuid,
    content: Content,
}

#[derive(Debug, Clone)]
enum Content {
    Track(Uuid),
    Playlist(Uuid),
}
