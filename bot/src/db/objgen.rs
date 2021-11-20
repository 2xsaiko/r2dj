use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::postgres::PgQueryResult;
use sqlx::{Executor, PgPool, Postgres};
use thiserror::Error;
use uuid::Uuid;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("The table was changed by someone else while editing, at {0}")]
    OutdatedState(DateTime<Utc>),
    #[error("Database error: {0}")]
    Sqlx(#[from] sqlx::Error),
}

#[derive(Default, Debug, Clone, PartialEq, Eq, Ord, PartialOrd)]
pub struct ObjectHeader {
    id: Option<Uuid>,
    modified: bool,
    created_at: Option<DateTime<Utc>>,
    modified_at: Option<DateTime<Utc>>,
}

impl ObjectHeader {
    pub fn from_loaded(
        id: Uuid,
        created_at: Option<DateTime<Utc>>,
        modified_at: Option<DateTime<Utc>>,
    ) -> Self {
        ObjectHeader {
            id: Some(id),
            modified: false,
            created_at,
            modified_at,
        }
    }

    pub fn id(&self) -> Option<Uuid> {
        self.id
    }

    pub fn persistent(&self) -> bool {
        self.id.is_some() && !self.modified
    }

    pub fn created_at(&self) -> Option<DateTime<Utc>> {
        self.created_at
    }

    pub fn modified_at(&self) -> Option<DateTime<Utc>> {
        self.modified_at
    }

    pub fn mark_changed(&mut self) {
        self.modified = true;
    }

    pub fn save(&mut self) -> Option<Save> {
        if self.id.is_some() && !self.modified {
            None
        } else {
            let now = Utc::now();

            Some(Save {
                id: self.id.unwrap_or_else(|| Uuid::new_v4()),
                header: self,
                now,
            })
        }
    }
}

pub struct Save<'a> {
    header: &'a mut ObjectHeader,
    id: Uuid,
    now: DateTime<Utc>,
}

impl<'a> Save<'a> {
    pub fn succeed(mut self) {
        if self.header.id.is_none() {
            self.header.created_at = Some(self.now);
        } else {
            self.header.modified_at = Some(self.now);
        }

        self.header.id = Some(self.id);
        self.header.modified = false;
    }

    pub fn is_new(&self) -> bool {
        self.header.id.is_none()
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn now(&self) -> DateTime<Utc> {
        self.now
    }

    pub fn header(&self) -> &ObjectHeader {
        &self.header
    }
}

/// Like `Clone`, but resets the metadata of the object so that saving the new
/// object creates a new row in the database.
pub trait Detach: Sized {
    fn detach(&self) -> Self;
}

#[async_trait]
pub trait Object: Sized {
    async fn load(id: Uuid, db: &PgPool) -> sqlx::Result<Self>;

    async fn save<'a, E>(&mut self, db: E) -> Result<PgQueryResult>
    where
        E: Executor<'a, Database = Postgres> + Copy;

    fn id(&self) -> Option<Uuid>;

    fn persistent(&self) -> bool;

    fn created_at(&self) -> Option<DateTime<Utc>>;

    fn modified_at(&self) -> Option<DateTime<Utc>>;
}

#[async_trait]
pub trait Entity {
    type Object: Object;

    async fn reload(&mut self, db: &PgPool) -> sqlx::Result<()>;

    async fn save(&mut self, db: &PgPool) -> Result<PgQueryResult>;

    fn object(&self) -> &Self::Object;
}

macro_rules! impl_detach {
    ($name:ident) => {
        impl $crate::db::objgen::Detach for $name {
            fn detach(&self) -> Self {
                $name {
                    header: Default::default(),
                    ..Clone::clone(self)
                }
            }
        }
    };
}

macro_rules! impl_object {
    () => {
        #[allow(unused)]
        pub fn id(&self) -> Option<Uuid> {
            self.header.id()
        }

        #[allow(unused)]
        pub fn persistent(&self) -> bool {
            self.header.persistent()
        }

        #[allow(unused)]
        pub fn created_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
            self.header.created_at()
        }

        #[allow(unused)]
        pub fn modified_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
            self.header.modified_at()
        }
    };
}

macro_rules! check_out_of_date {
    ($table:ident, $save:expr, $db:expr) => {
        // language=SQL
        let old_modified =
            sqlx::query!(concat!("SELECT modified FROM ", stringify!($table), " WHERE id = $1"), save.id())
                .fetch_one(&mut *$db)
                .await?
                .modified;

        match ($save.header().modified_at(), old_modified) {
            (Some(my_mtime), Some(db_mtime)) => {
                if db_mtime > my_mtime {
                    return Err(objgen::Error::OutdatedState(db_mtime));
                }
            }
            _ => {}
        }
    };
}