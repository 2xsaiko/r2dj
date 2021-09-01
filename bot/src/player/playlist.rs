use futures::future::BoxFuture;
use futures::{FutureExt, TryStreamExt};
use rand::Rng;
use sqlx::PgPool;
use uuid::Uuid;

use crate::player::Track;

#[derive(Debug, Clone)]
pub struct Playlist {
    persistent_id: Option<Uuid>,
    title: String,
    entries: Vec<PlaylistLike>,
    playlist_mode: PlaylistMode,
    shuffle: bool,
    last_played: Vec<usize>,
}

#[derive(Debug, Clone)]
pub enum PlaylistLike {
    Track(Track),
    Playlist(Playlist),
}

#[derive(PartialEq, Eq, Copy, Clone, Debug)]
pub enum PlaylistMode {
    Flatten,
    RoundRobin,
}

pub enum PlayMode {
    Once,
    Repeat,
    RepeatOne,
}

trait OrderTracker {}

struct RootOrderTracker {
    v: Vec<f32>,
}

struct SubOrderTracker<P> {
    parent: P,
    offset: usize,
    len: usize,
}

impl Playlist {
    pub fn new() -> Self {
        Playlist {
            persistent_id: None,
            title: "Playlist".to_string(),
            entries: vec![],
            playlist_mode: PlaylistMode::Flatten,
            shuffle: false,
            last_played: vec![],
        }
    }

    pub fn load(uuid: Uuid, db: &PgPool) -> BoxFuture<sqlx::Result<Self>> {
        async move {
            let row = sqlx::query!("SELECT title FROM playlist WHERE id = $1", uuid)
                .fetch_one(db)
                .await?;

            let db_entries: Vec<_> = sqlx::query!(
                "SELECT id, track, sub_playlist FROM playlist_entry \
                 WHERE playlist = $1 \
                 ORDER BY index",
                uuid
            )
            .fetch(db)
            .try_collect()
            .await?;

            let mut entries = Vec::new();

            for el in db_entries {
                let entry = match (el.track, el.sub_playlist) {
                    (Some(track), None) => PlaylistLike::Track(Track::load(track, db).await?),
                    (None, Some(sub_playlist)) => {
                        PlaylistLike::Playlist(Playlist::load(sub_playlist, db).await?)
                    }
                    (_, _) => unreachable!(),
                };
                entries.push(entry);
            }

            Ok(Playlist {
                persistent_id: Some(uuid),
                title: row.title,
                entries,
                playlist_mode: PlaylistMode::Flatten,
                shuffle: false,
                last_played: vec![],
            })
        }
        .boxed()
    }

    pub fn id(&self) -> Option<Uuid> {
        self.persistent_id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title<S>(&mut self, title: S)
    where
        S: Into<String>,
    {
        self.title = title.into();
    }

    pub fn entries(&self) -> &[PlaylistLike] {
        &self.entries
    }

    fn add(&mut self, entry: PlaylistLike) {
        self.entries.push(entry);
    }

    pub fn add_track(&mut self, track: Track) {
        self.add(PlaylistLike::Track(track));
    }

    pub fn add_playlist(&mut self, playlist: Playlist) {
        self.add(PlaylistLike::Playlist(playlist));
    }

    pub fn next(&mut self) -> Option<Track> {
        if !self.has_tracks() {
            None
        } else {
            // we have at least one track that we can play, so let's just keep trying
            loop {
                let r = self.pick_nth(
                    self.shuffle,
                    select_next(self.length(), &self.last_played, self.shuffle),
                );
                if let Some(r) = r {
                    break Some(r);
                }
            }
        }
    }

    fn pick_nth(&mut self, shuffled: bool, idx: usize) -> Option<Track> {
        let next = if self.shuffle && !shuffled {
            select_next_random(self.length(), &self.last_played)
        } else {
            idx
        };

        match self.playlist_mode {
            PlaylistMode::Flatten => {
                let lengths: Vec<_> = self.entries.iter().map(|el| el.length()).collect();
                let all_length_one = lengths.iter().all(|el| *el == 1);

                if all_length_one {
                    // optimization: if all the lengths are one, we can just
                    // select an index like for the round robin mode
                    self.add_play_last(next);
                    self.entries[next].next()
                } else {
                    let mut offset = 0;

                    let mut iter = lengths.iter().enumerate();
                    let (entry_idx, sub_idx) = loop {
                        let (idx, &sub_len) = iter.next().unwrap();

                        if next - offset < sub_len {
                            break (idx, next - offset);
                        }

                        offset += sub_len;
                    };

                    self.add_play_last(next);
                    self.entries[entry_idx].pick_nth(shuffled || self.shuffle, sub_idx)
                }
            }
            PlaylistMode::RoundRobin => {
                self.add_play_last(next);
                self.entries[next].next()
            }
        }
    }

    pub fn reset(&mut self) {
        self.last_played.clear();
        self.entries.iter_mut().for_each(|el| el.reset());
    }

    pub fn set_mode(&mut self, mode: PlaylistMode) {
        if self.playlist_mode != mode {
            self.playlist_mode = mode;
            self.reset();
        }
    }

    pub fn set_shuffle(&mut self, shuffle: bool) {
        self.shuffle = shuffle;
    }

    pub fn shuffle(&self) -> bool {
        self.shuffle
    }

    pub fn length(&self) -> usize {
        match self.playlist_mode {
            PlaylistMode::Flatten => self.entries.iter().map(|el| el.length()).sum(),
            PlaylistMode::RoundRobin => self.entries.len(),
        }
    }

    pub fn has_tracks(&self) -> bool {
        self.entries.iter().any(|el| el.has_tracks())
    }

    fn add_play_last(&mut self, idx: usize) {
        if let Some(idx_idx) = self.last_played.iter().position(|&el| el == idx) {
            self.last_played.copy_within(idx_idx + 1.., idx_idx);
            let i = self.last_played.len() - 1;
            self.last_played[i] = idx;
        } else {
            self.last_played.push(idx);
        }
    }
}

impl PlaylistLike {
    pub fn next(&mut self) -> Option<Track> {
        match self {
            PlaylistLike::Track(tr) => Some(tr.clone()),
            PlaylistLike::Playlist(pl) => pl.next(),
        }
    }

    pub fn pick_nth(&mut self, shuffled: bool, idx: usize) -> Option<Track> {
        match self {
            PlaylistLike::Track(tr) => {
                assert_eq!(0, idx);
                Some(tr.clone())
            }
            PlaylistLike::Playlist(pl) => pl.pick_nth(shuffled, idx),
        }
    }

    pub fn reset(&mut self) {
        match self {
            PlaylistLike::Track(_) => {}
            PlaylistLike::Playlist(pl) => pl.reset(),
        }
    }

    pub fn length(&self) -> usize {
        match self {
            PlaylistLike::Track(_) => 1,
            PlaylistLike::Playlist(pl) => pl.length(),
        }
    }

    pub fn has_tracks(&self) -> bool {
        match self {
            PlaylistLike::Track(_) => true,
            PlaylistLike::Playlist(pl) => pl.has_tracks(),
        }
    }

    pub fn shuffle(&self) -> bool {
        match self {
            PlaylistLike::Track(_) => false,
            PlaylistLike::Playlist(pl) => pl.shuffle,
        }
    }

    pub fn title(&self) -> Option<&str> {
        match self {
            PlaylistLike::Track(t) => t.title(),
            PlaylistLike::Playlist(pl) => Some(pl.title()),
        }
    }
}

impl RootOrderTracker {
    pub fn new(size: usize) -> Self {
        RootOrderTracker { v: vec![1.0; size] }
    }

    pub fn next(&mut self) -> usize {
        let next = 0;
        self.add(next);
        next
    }

    pub fn add(&mut self, idx: usize) {
        self.v.iter_mut().for_each(|v| *v *= 0.5);
        self.v[idx] += 10.0;
    }
}

fn select_next(len: usize, last: &[usize], random: bool) -> usize {
    if random {
        select_next_random(len, last)
    } else {
        select_next_sequential(len, last)
    }
}

fn select_next_sequential(len: usize, last: &[usize]) -> usize {
    if let Some(&last) = last.last() {
        (last + 1) % len
    } else {
        0
    }
}

fn select_next_random(len: usize, last: &[usize]) -> usize {
    assert!(len > 0);
    assert!(last.len() <= len);

    let unweighted = len - last.len();

    let max: f32 = unweighted as f32 + (1.0 - 2f32.powi(-(last.len() as i32)));
    let pick = rand::thread_rng().gen_range(0f32..=max);

    if pick < unweighted as f32 {
        let idx = pick.floor() as usize;
        (0..len).filter(|el| !last.contains(el)).nth(idx).unwrap()
    } else {
        let pick_rel = pick - unweighted as f32;
        let idx = (-(1.0 - pick_rel).log2()).floor() as usize;

        last[idx]
    }
}

fn sub_select_next_random(min: usize, max: usize, last: &[usize]) -> usize {
    let new_last: Vec<_> = last
        .iter()
        .filter(|&&el| el >= min && el < max)
        .map(|el| el - min)
        .collect();
    select_next_random(max - min, &new_last)
}

fn add_last(vec: &mut Vec<usize>, idx: usize) {
    if let Some(idx_idx) = vec.iter().position(|&el| el == idx) {
        vec.copy_within(idx_idx + 1.., idx_idx);
        let i = vec.len() - 1;
        vec[i] = idx;
    } else {
        vec.push(idx);
    }
}

#[cfg(test)]
mod test {
    use super::{add_last, select_next, Playlist, PlaylistLike, PlaylistMode, Track};

    #[test]
    fn test_random_dist() {
        let mut vec = Vec::new();

        println!("playing 100 tracks in sequence");
        for _ in 0..100 {
            let next = select_next(100, &vec, false);
            println!("{} ({:?})", next, vec.iter().position(|el| *el == next));
            add_last(&mut vec, next);
        }

        println!("playing 1000 tracks randomly");

        for _ in 0..1000 {
            let next = select_next(100, &vec, true);
            println!("{} ({:?})", next, vec.iter().position(|el| *el == next));
            add_last(&mut vec, next);
        }
    }

    #[test]
    fn test_playlist() {
        // let mut playlist_1 = Playlist::new();
        // playlist_1.set_shuffle(true);
        // for i in 0..100 {
        //     playlist_1.add_track(Track::new(format!("pl1/tr{}", i)));
        // }
        //
        // let mut playlist_2 = Playlist::new();
        // playlist_2.set_shuffle(true);
        // for i in 0..50 {
        //     playlist_2.add_track(Track::new(format!("pl2/tr{}", i)));
        // }
        //
        // let mut playlist_3 = Playlist::new();
        // playlist_3.set_shuffle(true);
        // for i in 0..20 {
        //     playlist_3.add_track(Track::new(format!("pl3/tr{}", i)));
        // }
        //
        // let mut playlist = Playlist::new();
        // playlist.set_shuffle(true);
        // playlist.set_mode(PlaylistMode::RoundRobin);
        // playlist.add(PlaylistLike::Playlist(playlist_1));
        // playlist.add(PlaylistLike::Playlist(playlist_2));
        // playlist.add(PlaylistLike::Playlist(playlist_3));
        //
        // for _ in 0..170 {
        //     println!("{:?}", playlist.next());
        // }
    }
}
