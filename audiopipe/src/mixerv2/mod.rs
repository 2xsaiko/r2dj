use std::io;
use std::mem::MaybeUninit;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use pin_project_lite::pin_project;
use tokio::sync::Mutex;
use tokio::time::interval;

use crate::streamio::StreamWrite;
use std::cmp::min;

pub trait AudioFormat {
    type Sample;
    const SAMPLE_RATE: u32;
}

struct Pcm16Bit48000;

impl AudioFormat for Pcm16Bit48000 {
    type Sample = i16;
    const SAMPLE_RATE: u32 = 48000;
}

struct PcmFloat48000;

impl AudioFormat for PcmFloat48000 {
    type Sample = f32;
    const SAMPLE_RATE: u32 = 48000;
}

pin_project! {
    pub struct AudioSource<F, S> {
        format: F,
        #[pin]
        stream: S,
    }
}

impl<F, S> StreamWrite<F::Sample> for AudioSource<F, S>
where
    F: AudioFormat,
    S: StreamWrite<F::Sample>,
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[F::Sample],
    ) -> Poll<io::Result<usize>> {
        self.project().stream.poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.project().stream.poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.project().stream.poll_close(cx)
    }
}

struct SourceBuffer<S> {
    buffer: Vec<S>,
    head: usize,
    tail: usize,
}

impl<S> SourceBuffer<S>
where
    S: Default + Clone,
{
    pub fn new(capacity: usize) -> Self {
        SourceBuffer {
            buffer: vec![S::default(); capacity],
            head: 0,
            tail: 0,
        }
    }
}

impl<S> SourceBuffer<S> {
    /// Returns how many slots are available to write into this buffer.
    pub fn free(&self) -> usize {
        if self.tail < self.head {
            self.head - self.tail
        } else {
            self.buffer.len() - self.head + self.tail
        }
    }

    /// Returns how many slots are available to read from this buffer
    pub fn available(&self) -> usize {
        if self.tail < self.head {
            self.buffer.len() - self.tail + self.head
        } else {
            self.tail - self.head
        }
    }
}

#[derive(Debug, Clone)]
pub struct Core {
    data: Arc<Mutex<CoreData>>,
}

#[derive(Debug, Clone, Default)]
struct CoreData {
}

pub const TICK_INTERVAL_MS: u64 = 50;

impl Core {
    pub fn new() -> Self {
        let c = Core {
            data: Arc::new(Mutex::new(CoreData::default())),
        };

        tokio::spawn(c.clone().run());

        c
    }

    async fn run(self) {
        let mut interval = interval(Duration::from_millis(TICK_INTERVAL_MS));

        loop {
            interval.tick().await;
            let data = self.data.lock().await;
        }
    }
}
