use std::fmt::{Display, Formatter};

use chrono::NaiveDate;
use sqlx::postgres::{PgArguments, PgRow};
use sqlx::{Arguments, FromRow, PgConnection, Row};
use uuid::Uuid;

use crate::db::objgen;
use crate::db::objgen::ObjectHeader;
use crate::fmt::HtmlDisplay;

#[derive(Clone, Debug, Default)]
pub struct Track {
    header: ObjectHeader,
    code: Option<String>,
    title: Option<String>,
    genre: Option<Uuid>,
    release_date: Option<NaiveDate>,
}

impl_detach!(Track);

impl Track {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn set_code(&mut self, code: impl Into<String>) {
        self.code = Some(code.into());
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
        let mut args = PgArguments::default();
        args.add(id);
        // language=SQL
        sqlx::query_as_with("SELECT * FROM track WHERE id = $1", args)
            .fetch_one(db)
            .await
    }

    pub async fn load_by_code(code: &str, db: &mut PgConnection) -> sqlx::Result<Self> {
        let mut args = PgArguments::default();
        args.add(code);
        // language=SQL
        sqlx::query_as_with(
            "SELECT * FROM track WHERE code = $1 AND deleted = FALSE",
            args,
        )
        .fetch_one(db)
        .await
    }

    pub async fn save(&mut self, db: &mut PgConnection) -> objgen::Result<()> {
        if let Some(save) = self.header.save() {
            if save.is_new() {
                // language=SQL
                let code = match &self.code {
                    None => {
                        sqlx::query_unchecked!(
                            "INSERT INTO track (id, code, title, genre, release_date, created, deleted) \
                             VALUES ($1, DEFAULT, $2, $3, $4, $5, $6) \
                             RETURNING code",
                            save.id(),
                            &self.title,
                            &self.genre,
                            &self.release_date,
                            save.now(),
                            save.deleted(),
                        )
                        .fetch_one(&mut *db)
                        .await?
                        .code
                    }
                    Some(code) => {
                        sqlx::query_unchecked!(
                            "INSERT INTO track (id, code, title, genre, release_date, created, deleted) \
                             VALUES ($1, $2, $3, $4, $5, $6, $7) \
                             RETURNING code",
                            save.id(),
                            code,
                            &self.title,
                            &self.genre,
                            &self.release_date,
                            save.now(),
                            save.deleted(),
                        )
                        .fetch_one(&mut *db)
                        .await?
                        .code
                    }
                };

                self.code = Some(code);
            } else {
                // language=SQL
                let db_status = sqlx::query!(
                    "SELECT modified, deleted FROM track WHERE id = $1",
                    save.id()
                )
                .fetch_one(&mut *db)
                .await?;

                if let (Some(my_mtime), Some(db_mtime)) =
                    (save.header().modified_at(), db_status.modified)
                {
                    if db_mtime > my_mtime {
                        return Err(objgen::Error::OutdatedState(db_mtime));
                    }
                }

                if db_status.deleted {
                    return Err(objgen::Error::Deleted);
                }

                sqlx::query_unchecked!(
                    // language=SQL
                    "UPDATE track \
                     SET code = $2, title = $3, genre = $4, release_date = $5, modified = $6 \
                     WHERE id = $1",
                    save.id(),
                    self.code.as_deref().expect("code must be set"),
                    &self.title,
                    &self.genre,
                    &self.release_date,
                    save.now(),
                )
                .execute(&mut *db)
                .await?;
            };

            save.succeed();
        }

        Ok(())
    }

    pub async fn delete(&mut self, db: &mut PgConnection) -> objgen::Result<()> {
        self.header.mark_deleted();
        self.save(db).await
    }
}

impl<'r> FromRow<'r, PgRow> for Track {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let header = ObjectHeader::from_row(row)?;
        let code = row.try_get("code")?;
        let title = row.try_get("title")?;
        let genre = row.try_get("genre")?;
        let release_date = row.try_get("release_date")?;

        Ok(Track {
            header,
            code: Some(code),
            title,
            genre,
            release_date,
        })
    }
}

impl Display for Track {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(
            f,
            "{} {}",
            self.code.as_deref().unwrap_or(""),
            self.title.as_deref().unwrap_or("Unnamed Track")
        )
    }
}

impl HtmlDisplay for Track {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(
            f,
            "<code>{}</code> {}",
            self.code.as_deref().unwrap_or(""),
            self.title.as_deref().unwrap_or("Unnamed Track")
        )
    }
}
