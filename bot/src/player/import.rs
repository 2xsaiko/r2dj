use chrono::Utc;
use sqlx::{PgPool, Postgres, Transaction};
use thiserror::Error;
use uuid::Uuid;
use youtube_dl::{Playlist, SingleVideo, YoutubeDl, YoutubeDlOutput};

use crate::db::types::ExternalSource;

pub async fn create_yt_playlist(playlist_id: &str, db: &PgPool) -> Result<Uuid> {
    let pd = get_playlist_data(playlist_id)?;
    let title = pd.title.as_deref().unwrap_or("Imported Playlist");

    let id = Uuid::new_v4();
    let now = Utc::now();

    let mut ta = db.begin().await?;

    sqlx::query!(
        "INSERT INTO playlist (id, title, external_source_type, external_source, created) \
         VALUES ($1, $2, 'youtube', $3, $4)",
        id,
        title,
        playlist_id,
        now
    )
    .execute(&mut ta)
    .await?;

    do_update_yt_playlist(&id, pd, &mut ta).await?;
    ta.commit().await?;

    Ok(id)
}

pub async fn update_playlist<E>(id: &Uuid, db: &PgPool) -> Result<()> {
    let q = sqlx::query!(
        r#"SELECT
               external_source_type as "external_source_type: ExternalSource",
               external_source
           FROM playlist
           WHERE playlist.id = $1"#,
        id,
    )
    .fetch_one(db)
    .await?;

    let (t, src) = match (q.external_source_type, q.external_source) {
        (Some(t), Some(src)) => (t, src),
        (_, _) => return Ok(()),
    };

    assert_eq!(ExternalSource::Youtube, t);

    let pd = get_playlist_data(&src)?;

    let mut ta = db.begin().await?;
    do_update_yt_playlist(id, pd, &mut ta).await?;
    ta.commit().await?;

    Ok(())
}

async fn do_update_yt_playlist(
    id: &Uuid,
    playlist: Box<Playlist>,
    db: &mut Transaction<'_, Postgres>,
) -> Result<()> {
    // TODO: don't be as destructive
    sqlx::query!("DELETE FROM playlist_entry WHERE playlist = $1", id)
        .execute(&mut *db)
        .await?;

    let entries = match playlist.entries {
        None => return Err(Error::EmptyPlaylist),
        Some(v) => v,
    };

    for (idx, el) in entries.iter().enumerate() {
        let track = get_or_create_yt_track(&el, &mut *db).await?;

        sqlx::query!(
            "INSERT INTO playlist_entry (id, playlist, index, track) \
             VALUES ($1, $2, $3, $4)",
            Uuid::new_v4(),
            id,
            idx as u32,
            track,
        )
        .execute(&mut *db)
        .await?;
    }

    Ok(())
}

async fn get_or_create_yt_track(
    video_meta: &SingleVideo,
    db: &mut Transaction<'_, Postgres>,
) -> Result<Uuid> {
    let existing = sqlx::query!(
        "SELECT t.id FROM track t \
         INNER JOIN track_provider tp ON tp.track = t.id \
         WHERE tp.type = 'youtube' AND tp.source = $1",
        &video_meta.id
    )
    .fetch_optional(&mut *db)
    .await?;

    if let Some(existing) = existing {
        Ok(existing.id)
    } else {
        let id = Uuid::new_v4();

        sqlx::query!(
            "INSERT INTO track (id, title) \
             VALUES ($1, $2)",
            id,
            video_meta.title,
        )
        .execute(&mut *db)
        .await?;

        sqlx::query!(
            "INSERT INTO track_provider (id, track, type, source) \
             VALUES ($1, $2, 'youtube', $3)",
            Uuid::new_v4(),
            id,
            video_meta.id,
        )
        .execute(&mut *db)
        .await?;

        Ok(id)
    }
}

fn get_playlist_title(playlist_id: &str) -> Result<String, youtube_dl::Error> {
    Ok(get_playlist_data(playlist_id)?.title.unwrap())
}

fn get_playlist_data(playlist_id: &str) -> Result<Box<Playlist>, youtube_dl::Error> {
    let output = YoutubeDl::new(format!(
        "https://www.youtube.com/playlist?list={}",
        playlist_id
    ))
    .flat_playlist(true)
    .run()?;
    match output {
        YoutubeDlOutput::Playlist(pl) => Ok(pl),
        YoutubeDlOutput::SingleVideo(_) => unreachable!(),
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("youtube-dl error: {0}")]
    YoutubeDl(#[from] youtube_dl::Error),
    #[error("Playlist info without entries")]
    EmptyPlaylist,
    #[error("Database error: {0}")]
    Sqlx(#[from] sqlx::Error),
}

#[cfg(test)]
mod test {
    use crate::player::import::get_playlist_title;

    #[test]
    fn test_playlist() {
        assert_eq!(
            "Interface",
            get_playlist_title("PLPnjato8iGXLQbppBPhOny8XLSRl7S5pM").unwrap()
        );
    }
}
