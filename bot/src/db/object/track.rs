use chrono::NaiveDate;
use sqlx::postgres::PgQueryResult;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::db::objgen;
use crate::db::objgen::ObjectHeader;

#[derive(Clone, Debug, Default)]
pub struct Track {
    header: ObjectHeader,
    title: Option<String>,
    genre: Option<Uuid>,
    release_date: Option<NaiveDate>,
}

impl_detach!(Track);

impl Track {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn set_title(&mut self, title: Option<String>) {
        self.header.mark_changed();
        self.title = title;
    }

    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub fn set_genre(&mut self, genre: Option<Uuid>) {
        self.header.mark_changed();
        self.genre = genre;
    }

    pub fn genre(&self) -> Option<Uuid> {
        self.genre
    }

    pub fn set_release_date(&mut self, release_date: Option<NaiveDate>) {
        self.header.mark_changed();
        self.release_date = release_date;
    }

    pub fn release_date(&self) -> Option<NaiveDate> {
        self.release_date
    }
}

impl Track {
    impl_object!();

    pub async fn load(id: Uuid, db: &mut PgConnection) -> sqlx::Result<Self> {
        // language=SQL
        let row = sqlx::query!(
            "SELECT title, genre, release_date, created, modified
             FROM track WHERE id = $1",
            id
        )
        .fetch_one(db)
        .await?;

        Ok(Track {
            header: ObjectHeader::from_loaded(id, row.created, row.modified),
            title: row.title,
            genre: row.genre,
            release_date: row.release_date,
        })
    }

    pub async fn save(&mut self, db: &mut PgConnection) -> objgen::Result<PgQueryResult> {
        if let Some(save) = self.header.save() {
            let r = if save.is_new() {
                // language=SQL
                sqlx::query_unchecked!(
                    "INSERT INTO track (id, title, genre, release_date, created)
                     VALUES ($1, $2, $3, $4, $5)",
                    save.id(),
                    &self.title,
                    &self.genre,
                    &self.release_date,
                    save.now(),
                )
                .execute(&mut *db)
                .await?
            } else {
                // language=SQL
                let old_modified =
                    sqlx::query!("SELECT modified FROM track WHERE id = $1", save.id())
                        .fetch_one(&mut *db)
                        .await?
                        .modified;

                match (save.header().modified_at(), old_modified) {
                    (Some(my_mtime), Some(db_mtime)) => {
                        if db_mtime > my_mtime {
                            return Err(objgen::Error::OutdatedState(db_mtime));
                        }
                    }
                    _ => {}
                }

                // language=SQL
                sqlx::query_unchecked!(
                    "UPDATE track
                     SET title = $2, genre = $3, release_date = $4, modified = $5
                     WHERE id = $1",
                    save.id(),
                    &self.title,
                    &self.genre,
                    &self.release_date,
                    save.now(),
                )
                .execute(&mut *db)
                .await?
            };

            save.succeed();

            Ok(r)
        } else {
            Ok(Default::default())
        }
    }
}
