use std::collections::HashMap;

use log::debug;

use msgtools::Ac;

use crate::db::entity::playlist::Content;
use crate::db::entity::{Playlist, Track};
use crate::db::object::playlist::NestingMode;
use crate::player::playlistv2::treepath::{TreePath, TreePathBuf};

pub mod treepath;

#[derive(Debug, Clone)]
pub struct PlaylistTracker {
    playlist: Ac<Playlist>,
    trackers: HashMap<TreePathBuf, Vec<(u16, TreePathBuf)>>,
    iteration: u16,
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum GetTrackError {
    End,
    NoTracks,
}

impl PlaylistTracker {
    pub fn new(playlist: Ac<Playlist>) -> Self {
        PlaylistTracker {
            playlist,
            trackers: HashMap::new(),
            iteration: 0,
        }
    }

    pub fn restart(&mut self) {
        self.iteration = self.iteration.overflowing_add(1).0;
    }

    pub fn next(&mut self) -> Result<&Track, GetTrackError> {
        let mut available = Vec::new();
        self.collect_choices(&TreePathBuf::root(), &self.playlist, |x| available.push(x));

        if available.is_empty() {
            Err(GetTrackError::NoTracks)
        } else {
            let last_played = self
                .trackers
                .get(&TreePathBuf::root())
                .map(|x| &**x)
                .unwrap_or(&[]);
            let is_random = false; // TODO

            let next_idx = if is_random {
                todo!()
            } else {
                match last_played
                    .last()
                    .filter(|(iteration, _)| *iteration == self.iteration)
                    .and_then(|(_, path)| available.iter().position(|el| el == path))
                {
                    None => Some(&available[0]),
                    Some(idx) => available.get(idx + 1),
                }
            };

            if let Some(next_idx) = next_idx {
                self.insert_last_played(&TreePathBuf::root(), &next_idx);
            }

            next_idx
                .and_then(move |x| self.playlist.get_track(x))
                .ok_or(GetTrackError::End)
        }
    }

    fn collect_choices(&self, pl_path: &TreePath, pl: &Playlist, mut f: impl FnMut(TreePathBuf)) {
        for (idx, e) in pl.entries().iter().enumerate() {
            let new_path = pl_path.join(&[idx as u32]);

            match e.content() {
                Content::Track(_) => {
                    f(new_path);
                }
                Content::Playlist(pl1) => match pl.object().nesting_mode() {
                    NestingMode::Flatten => {
                        // prevent unlimited codegen recursion
                        let b: Box<&mut dyn FnMut(_)> = Box::new(&mut f);
                        self.collect_choices(&new_path, pl1, b);
                    }
                    NestingMode::RoundRobin => {
                        if !self.is_empty_(pl) {
                            f(new_path);
                        }
                    }
                },
            }
        }
    }

    fn available(&self, at: &TreePath) -> usize {
        let pl = self.playlist.get_playlist(at).expect("invalid path");
        let mut buf = at.to_tree_path_buf();

        match pl.object().nesting_mode() {
            NestingMode::Flatten => {
                let mut len = 0;

                for i in 0..pl.entries().len() {
                    buf.push_index(i as u32);
                    len += self.available(&buf);
                    buf.pop_index();
                }

                len
            }
            NestingMode::RoundRobin => {
                let mut len = 0;

                for i in 0..pl.entries().len() {
                    buf.push_index(i as u32);

                    if !self.is_empty(&buf) {
                        len += 1;
                    }

                    buf.pop_index();
                }

                len
            }
        }
    }

    fn is_empty(&self, at: &TreePath) -> bool {
        let pl = self.playlist.get_playlist(at).expect("invalid path");

        self.is_empty_(pl)
    }

    fn is_empty_(&self, pl: &Playlist) -> bool {
        for el in pl.entries().iter() {
            match el.content() {
                Content::Track(_) => return false,
                Content::Playlist(pl) => {
                    if !self.is_empty_(&pl) {
                        return false;
                    }
                }
            }
        }

        true
    }

    fn add_to_last_played(&mut self, track: &TreePath) {
        let mut depth = 1;
        let mut top = 0;

        while depth < track.len() - 1 {
            let current_pl = match self.playlist.get_playlist(&track[..depth]) {
                None => {
                    debug!("called add_to_last_played with invalid track path");
                    return;
                }
                Some(pl) => pl,
            };

            match current_pl.object().nesting_mode() {
                NestingMode::Flatten => {
                    // nothing
                }
                NestingMode::RoundRobin => {
                    self.insert_last_played(&track[..top], &track[..depth]);
                    top = depth;
                }
            }

            depth += 1;
        }

        self.insert_last_played(&track[..top], track);
    }

    fn insert_last_played(&mut self, context_tn: &TreePath, entry: &TreePath) {
        let vec = self
            .trackers
            .entry(context_tn.to_owned())
            .or_insert(Vec::new());

        if let Some(idx) = vec.iter().position(|(_, el)| &**el == entry) {
            let (_, it) = vec.remove(idx);
            vec.push((self.iteration, it));
        } else {
            vec.push((self.iteration, entry.to_owned()));
        }
    }

    pub fn add_track(&mut self, track: Track, parent: impl AsRef<TreePath>) -> Result<(), Track> {
        self.playlist.add_track(track, parent)
    }

    pub fn add_playlist(
        &mut self,
        playlist: Playlist,
        parent: impl AsRef<TreePath>,
    ) -> Result<(), Playlist> {
        self.playlist.add_playlist(playlist, parent)
    }

    pub fn playlist(&self) -> &Ac<Playlist> {
        &self.playlist
    }
}

struct TrackIterator<'a> {
    current: Option<TreePathBuf>,
    playlist: &'a Playlist,
}

impl<'a> Iterator for TrackIterator<'a> {
    type Item = TreePathBuf;

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.current.as_mut()?;

        loop {
            current.increment_last();

            match self.playlist.get_entry(&current) {
                None => {
                    current.pop_index();
                }
                Some(Content::Playlist(_)) => {
                    current.push_index(0);
                }
                Some(Content::Track(_)) => break Some(current.to_owned()),
            }

            if current.is_empty() {
                break None;
            }
        }
    }
}
