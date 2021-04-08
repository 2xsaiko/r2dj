use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use futures::future::BoxFuture;
use futures::FutureExt;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::process::{ChildStdin, ChildStdout, Command};

use crate::connect;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct FfmpegConfig {
    channels: u32,
    input_format: Format,
    output_format: Format,
    start_at: Duration,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Format {
    Auto,
    Pcm16BitLe(u32),
    Pcm16BitBe(u32),
}

pub async fn ffpipe<'a, I, O>(input: I, output: O, config: FfmpegConfig) -> io::Result<ExitStatus>
where
    I: TranscoderInput<'a>,
    O: TranscoderOutput<'a>,
{
    let mut ffmpeg = Command::new("ffmpeg");
    ffmpeg.arg("-nostdin");

    ffmpeg.arg("-ss");
    ffmpeg.arg(format!("{}", config.start_at.as_secs()));

    config.input_format.add_args(&mut ffmpeg);

    ffmpeg.arg("-i");
    ffmpeg.arg(input.to_arg());

    ffmpeg.arg("-ac");
    ffmpeg.arg(format!("{}", config.channels));

    config.output_format.add_args(&mut ffmpeg);

    ffmpeg.arg(output.to_arg());

    input.pre_spawn(&mut ffmpeg);
    output.pre_spawn(&mut ffmpeg);

    let mut handle = ffmpeg.spawn()?;

    let stdin = handle.stdin.take();
    let stdin_fut = async {
        match stdin {
            Some(stdin) => input.handle_stdin(stdin).await,
            None => Ok(()),
        }
    };

    let stdout = handle.stdout.take();
    let stdout_fut = async {
        match stdout {
            Some(stdout) => output.handle_stdout(stdout).await,
            None => Ok(()),
        }
    };

    let (r, _, _) = tokio::try_join!(handle.wait(), stdin_fut, stdout_fut)?;

    Ok(r)
}

pub trait TranscoderInput<'a>: Sized {
    fn to_arg(&self) -> &OsStr;

    fn pre_spawn(&self, command: &mut Command) {}

    fn handle_stdin(self, stdout: ChildStdin) -> BoxFuture<'a, io::Result<()>> {
        async { Ok(()) }.boxed()
    }
}

pub struct PathSource<T> {
    path: T,
}

impl<T> PathSource<T> {
    pub fn new(path: T) -> Self {
        PathSource { path }
    }
}

pub struct PipeSource<T> {
    pipe: T,
}

pub trait TranscoderOutput<'a>: Sized {
    fn to_arg(&self) -> &OsStr;

    fn pre_spawn(&self, command: &mut Command) {}

    fn handle_stdout(self, stdout: ChildStdout) -> BoxFuture<'a, io::Result<()>> {
        async { Ok(()) }.boxed()
    }
}

pub struct PathDest<T> {
    path: T,
}

pub struct PipeDest<T> {
    pipe: T,
}

impl<T> PipeDest<T> {
    pub fn new(pipe: T) -> Self {
        PipeDest { pipe }
    }
}

impl FfmpegConfig {
    pub fn channels(mut self, channels: u32) -> Self {
        self.channels = channels;
        self
    }

    pub fn input_format(mut self, input_format: Format) -> Self {
        self.input_format = input_format;
        self
    }

    pub fn output_format(mut self, output_format: Format) -> Self {
        self.output_format = output_format;
        self
    }

    pub fn start_at(mut self, start_at: Duration) -> Self {
        self.start_at = start_at;
        self
    }
}

impl Default for FfmpegConfig {
    fn default() -> Self {
        FfmpegConfig {
            channels: 1,
            input_format: Default::default(),
            output_format: Default::default(),
            start_at: Default::default(),
        }
    }
}

impl Format {
    #[cfg(target_endian = "little")]
    pub fn native_pcm(bitrate: u32) -> Self {
        Format::Pcm16BitLe(bitrate)
    }

    #[cfg(target_endian = "big")]
    pub fn native_pcm(bitrate: u32) -> Self {
        Format::Pcm16BitBe(bitrate)
    }

    fn add_args(&self, command: &mut Command) {
        match self {
            Format::Auto => {}
            Format::Pcm16BitLe(bitrate) => {
                command.args(&["-f", "s16le", "-ar"]);
                command.arg(format!("{}", bitrate));
            }
            Format::Pcm16BitBe(bitrate) => {
                command.args(&["-f", "s16be", "-ar"]);
                command.arg(format!("{}", bitrate));
            }
        }
    }
}

impl Default for Format {
    fn default() -> Self {
        Format::Auto
    }
}

impl<'a, T> TranscoderInput<'a> for PathSource<T>
where
    T: AsRef<Path>,
{
    fn to_arg(&self) -> &OsStr {
        self.path.as_ref().as_os_str()
    }
}

impl<'a, T> TranscoderInput<'a> for PipeSource<T>
where
    T: AsyncRead + Unpin + Send + 'a,
{
    fn to_arg(&self) -> &OsStr {
        OsStr::new("-")
    }

    fn pre_spawn(&self, command: &mut Command) {
        command.stdin(Stdio::piped());
    }

    fn handle_stdin(self, stdin: ChildStdin) -> BoxFuture<'a, io::Result<()>> {
        connect(self.pipe, stdin).boxed()
    }
}

impl<'a, T> TranscoderOutput<'a> for PathDest<T>
where
    T: AsRef<Path>,
{
    fn to_arg(&self) -> &OsStr {
        self.path.as_ref().as_os_str()
    }
}

impl<'a, T> TranscoderOutput<'a> for PipeDest<T>
where
    T: AsyncWrite + Unpin + Send + 'a,
{
    fn to_arg(&self) -> &OsStr {
        OsStr::new("-")
    }

    fn pre_spawn(&self, command: &mut Command) {
        command.stdout(Stdio::piped());
    }

    fn handle_stdout(self, stdout: ChildStdout) -> BoxFuture<'a, io::Result<()>> {
        connect(stdout, self.pipe).boxed()
    }
}
