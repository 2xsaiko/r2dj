use std::fmt::{Display, Formatter};

use sqlx::postgres::{PgArguments, PgRow};
use sqlx::{Arguments, FromRow, PgConnection, Row};
use uuid::Uuid;

use crate::db::objgen::{self, ObjectHeader};
use crate::fmt::HtmlDisplay;

#[derive(Clone, Default, Debug)]
pub struct Playlist {
    header: ObjectHeader,
    code: Option<String>,
    title: String,
    spotify_id: Option<String>,
    youtube_id: Option<String>,
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub enum NestingMode {
    Flatten,
    RoundRobin,
}

impl_detach!(Playlist);

impl Playlist {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn set_code(&mut self, code: impl Into<String>) {
        self.header.mark_changed();
        self.code = Some(code.into());
    }

    pub fn code(&self) -> Option<&str> {
        self.code.as_deref()
    }

    pub fn set_title(&mut self, title: impl Into<String>) {
        self.header.mark_changed();
        self.title = title.into();
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_spotify_id(&mut self, spotify_id: Option<impl Into<String>>) {
        self.header.mark_changed();
        self.spotify_id = spotify_id.map(|el| el.into());
        self.youtube_id = None;
    }

    pub fn spotify_id(&self) -> Option<&str> {
        self.spotify_id.as_deref()
    }

    pub fn set_youtube_id(&mut self, youtube_id: Option<impl Into<String>>) {
        self.header.mark_changed();
        self.spotify_id = None;
        self.youtube_id = youtube_id.map(|el| el.into());
    }

    pub fn youtube_id(&self) -> Option<&str> {
        self.youtube_id.as_deref()
    }

    pub fn set_nesting_mode(&mut self, _nesting_mode: NestingMode) {
        todo!()
    }

    pub fn nesting_mode(&self) -> NestingMode {
        NestingMode::Flatten // TODO
    }
}

impl Playlist {
    impl_object!();

    pub async fn load(id: Uuid, db: &mut PgConnection) -> sqlx::Result<Self> {
        let mut args = PgArguments::default();
        args.add(id);
        // language=SQL
        sqlx::query_as_with("SELECT * FROM playlist WHERE id = $1", args)
            .fetch_one(db)
            .await
    }

    pub async fn load_by_code(code: &str, db: &mut PgConnection) -> sqlx::Result<Self> {
        let mut args = PgArguments::default();
        args.add(code);
        // language=SQL
        sqlx::query_as_with(
            "SELECT * FROM playlist WHERE code = $1 AND deleted = FALSE",
            args,
        )
        .fetch_one(db)
        .await
    }

    pub async fn load_by_youtube_id(id: &str, db: &mut PgConnection) -> sqlx::Result<Self> {
        // language=SQL
        let row = sqlx::query!(
            "SELECT id, code, title, created, modified \
             FROM playlist \
             WHERE youtube_id = $1 AND deleted = false",
            id,
        )
        .fetch_one(db)
        .await?;

        Ok(Playlist {
            header: ObjectHeader::from_loaded(row.id, row.created, row.modified, false),
            code: Some(row.code),
            title: row.title,
            spotify_id: None,
            youtube_id: Some(id.to_string()),
        })
    }

    pub async fn save(&mut self, db: &mut PgConnection) -> objgen::Result<()> {
        // using unchecked queries because it wants non-Option spotify_id/youtube_id

        if let Some(save) = self.header.save() {
            if save.is_new() {
                // TODO macro for this, damn you SQL for not letting me pass DEFAULT as a query param >:(
                // language=SQL
                let code = match &self.code {
                    None => {
                        sqlx::query_unchecked!(
                            "INSERT INTO playlist (id, code, title, spotify_id, youtube_id, created, deleted) \
                             VALUES ($1, DEFAULT, $2, $3, $4, $5, $6) \
                             RETURNING code",
                            save.id(),
                            &self.title,
                            &self.spotify_id,
                            &self.youtube_id,
                            save.now(),
                            save.deleted(),
                        )
                        .fetch_one(&mut *db)
                        .await?
                        .code
                    }
                    Some(code) => {
                        sqlx::query_unchecked!(
                            "INSERT INTO playlist (id, code, title, spotify_id, youtube_id, created, deleted) \
                             VALUES ($1, $2, $3, $4, $5, $6, $7) \
                             RETURNING code",
                            save.id(),
                            code,
                            &self.title,
                            &self.spotify_id,
                            &self.youtube_id,
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
                    "SELECT modified, deleted FROM playlist WHERE id = $1",
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
                    "UPDATE playlist \
                     SET code = $2, title = $3, spotify_id = $4, youtube_id = $5, modified = $6, deleted = $7 \
                     WHERE id = $1",
                    save.id(),
                    self.code.as_deref().expect("code must be set"),
                    &self.title,
                    &self.spotify_id,
                    &self.youtube_id,
                    save.now(),
                    save.deleted(),
                )
                .execute(&mut *db)
                .await?;
            }

            save.succeed();
        }

        Ok(())
    }

    pub async fn delete(&mut self, db: &mut PgConnection) -> objgen::Result<()> {
        self.header.mark_deleted();
        self.save(db).await
    }
}

impl<'r> FromRow<'r, PgRow> for Playlist {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let header = ObjectHeader::from_row(row)?;
        let code = row.try_get("code")?;
        let title = row.try_get("title")?;
        let spotify_id = row.try_get("spotify_id")?;
        let youtube_id = row.try_get("youtube_id")?;

        Ok(Playlist {
            header,
            code: Some(code),
            title,
            spotify_id,
            youtube_id,
        })
    }
}

impl Display for Playlist {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(f, "{} {}", self.code.as_deref().unwrap_or(""), self.title)
    }
}

impl HtmlDisplay for Playlist {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(
            f,
            "<code>{}</code> {}",
            self.code.as_deref().unwrap_or(""),
            self.title
        )
    }
}
