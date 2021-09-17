use crate::db::objgen::ObjectHeader;
use uuid::Uuid;
use chrono::{DateTime, Utc};

#[derive(Clone, Debug, Default)]
pub struct Track {
    header: ObjectHeader,
    title: Option<String>,
    genre: Option<Uuid>,
    release_date: Option<DateTime<Utc>>,
}
