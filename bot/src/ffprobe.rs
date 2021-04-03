use std::io;
use std::io::Cursor;
use std::path::Path;
use std::process::Command;
use std::process::ExitStatus;
use std::time::Duration;

use serde::Deserialize;
use thiserror::Error;

use str_wrapped::StrWrapped;

pub fn ffprobe<P: AsRef<Path>>(path: P) -> Result<FileInfo> {
    let mut cmd = Command::new("ffprobe");
    cmd.args(&[
        "-v",
        "error",
        "-hide_banner",
        "-show_format",
        "-show_streams",
        "-print_format",
        "json",
    ]);
    cmd.arg(path.as_ref());
    let output = cmd.output()?;
    if output.status.success() {
        let fi: FileInfo = serde_json::from_reader(Cursor::new(&output.stdout))?;
        Ok(fi)
    } else {
        Err(Error::Ffprobe(
            output.status,
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ))
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("Parse error: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("FFmpeg error: {1} ({0})")]
    Ffprobe(ExitStatus, String),
}

#[derive(Deserialize, Debug, Clone)]
pub struct FileInfo {
    format: Format,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Format {
    duration: StrWrapped<f32>,
    bit_rate: Option<StrWrapped<u32>>,
    tags: Tags,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Tags {
    track: Option<StrWrapped<u32>>,
    artist: Option<String>,
    album: Option<String>,
    title: Option<String>,
    #[serde(rename = "TBPM")]
    tbpm: Option<StrWrapped<u32>>,
    genre: Option<String>,
    #[serde(rename = "TSRC")]
    tsrc: Option<String>,
}

impl FileInfo {
    pub fn duration(&self) -> Duration {
        Duration::from_secs_f32(*self.format.duration)
    }

    pub fn title(&self) -> Option<&str> {
        self.format.tags.title.as_deref()
    }

    pub fn artist(&self) -> Option<&str> {
        self.format.tags.artist.as_deref()
    }

    pub fn album(&self) -> Option<&str> {
        self.format.tags.album.as_deref()
    }

    pub fn track_index(&self) -> Option<u32> {
        self.format.tags.track.as_deref().cloned()
    }
}

mod str_wrapped {
    use std::borrow::Cow;
    use std::fmt;
    use std::fmt::{Display, Formatter};
    use std::ops::{Deref, DerefMut};
    use std::str::FromStr;

    use serde::de::Error;
    use serde::{Deserialize, Deserializer};

    // ffmpeg, please output proper JSON so I don't have to do this crap, thanks
    #[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
    pub struct StrWrapped<T> {
        parsed: T,
    }

    impl<T> Deref for StrWrapped<T> {
        type Target = T;

        fn deref(&self) -> &Self::Target {
            &self.parsed
        }
    }

    impl<T> DerefMut for StrWrapped<T> {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.parsed
        }
    }

    impl<'de, T> Deserialize<'de> for StrWrapped<T>
    where
        T: FromStr,
        T::Err: Display,
    {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            let s: Cow<str> = Deserialize::deserialize(deserializer)?;
            match s.parse() {
                Ok(v) => Ok(StrWrapped { parsed: v }),
                Err(e) => Err(D::Error::custom(e)),
            }
        }
    }

    impl<T> Display for StrWrapped<T>
    where
        T: Display,
    {
        fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
            self.parsed.fmt(f)
        }
    }
}
