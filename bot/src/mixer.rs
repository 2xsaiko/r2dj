use std::cmp::min;
use std::collections::HashMap;
use std::io::ErrorKind;
use std::ops::Add;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use bytes::{Buf, BytesMut};
use futures::task::{Context, Poll, Waker};
use futures::FutureExt;
use log::debug;
use tokio::io;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::time::{sleep_until, Sleep};
use tokio::time::{Duration, Instant};

use crate::util::slice_to_u8;

const FREQUENCY: u32 = 48000;
const WORD_SIZE: u32 = 2; // we're dealing with 16-bit PCM
const CHANNELS: u32 = 1;
const BUFFER_MS: u32 = 500;
const BUFFER_SIZE: usize = (FREQUENCY * WORD_SIZE * CHANNELS * BUFFER_MS / 1000) as usize;

struct Shared {
    buffers: HashMap<usize, SharedInput>,
    read_notify: Option<Waker>,
    last_read: Instant,
    sleep: Option<Pin<Box<Sleep>>>,
    next_input: usize,
}

struct SharedInput {
    write_notify: Option<Waker>,
    buffer: BytesMut,
    closed: bool,
}

pub struct MixerInput {
    shared: Arc<Mutex<Shared>>,
    id: usize,
}

pub struct MixerOutput {
    shared: Arc<Mutex<Shared>>,
}

pub fn new_mixer() -> (MixerInput, MixerOutput) {
    let shared = Arc::new(Mutex::new(Shared {
        buffers: HashMap::new(),
        read_notify: None,
        sleep: None,
        last_read: Instant::now(),
        next_input: 0,
    }));
    let output = MixerOutput {
        shared: shared.clone(),
    };
    let input = create_input(shared);

    (input, output)
}

fn create_input(shared: Arc<Mutex<Shared>>) -> MixerInput {
    let mut s = shared.lock().unwrap();
    let id = s.next_input;
    s.next_input += 1;
    let shared_input = SharedInput {
        write_notify: None,
        buffer: BytesMut::with_capacity(BUFFER_SIZE),
        closed: false,
    };
    s.buffers.insert(id, shared_input);
    drop(s);
    debug!("new input {}", id);
    MixerInput { shared, id }
}

impl Clone for MixerInput {
    fn clone(&self) -> Self {
        create_input(self.shared.clone())
    }
}

impl AsyncWrite for MixerInput {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let mut shared = self.shared.lock().unwrap();
        let shared_input = shared.buffers.get_mut(&self.id).unwrap();

        if shared_input.closed {
            Poll::Ready(Err(io::Error::new(ErrorKind::BrokenPipe, "Broken pipe")))
        } else if shared_input.buffer.len() < BUFFER_SIZE {
            let to_write = min(buf.len(), BUFFER_SIZE - shared_input.buffer.len());
            shared_input.buffer.extend_from_slice(&buf[..to_write]);

            if let Some(w) = shared.read_notify.take() {
                w.wake();
            }

            Poll::Ready(Ok(to_write))
        } else {
            shared_input.write_notify = Some(cx.waker().clone());
            Poll::Pending
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        let mut shared = self.shared.lock().unwrap();
        let shared_input = shared.buffers.get_mut(&self.id).unwrap();

        shared_input.closed = true;

        Poll::Ready(Ok(()))
    }
}

impl Drop for MixerInput {
    fn drop(&mut self) {
        let mut shared = self.shared.lock().unwrap();
        let shared_input = shared.buffers.get_mut(&self.id).unwrap();

        shared_input.closed = true;
    }
}

impl AsyncRead for MixerOutput {
    #[cfg(target_endian = "little")]
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let mut shared = self.shared.lock().unwrap();

        if let Some(sleep) = &mut shared.sleep {
            match sleep.poll_unpin(cx) {
                Poll::Ready(_) => {
                    shared.sleep = None;
                }
                Poll::Pending => {
                    return Poll::Pending;
                }
            }
        }

        shared.cleanup();

        if shared.buffers.is_empty() {
            // EOF (all inputs are closed)
            Poll::Ready(Ok(()))
        } else {
            let now = Instant::now();

            let time_diff = now.duration_since(shared.last_read);
            let max_samples = (FREQUENCY * CHANNELS * time_diff.as_millis() as u32 / 1000) as usize;

            if max_samples == 0 {
                // when can we get the next sample?
                let next_sample_time = now
                    .checked_add(Duration::from_micros(1000000 / FREQUENCY as u64))
                    .expect("Failed to add duration");
                let mut sleep = Box::pin(sleep_until(next_sample_time));
                if sleep.poll_unpin(cx).is_pending() {
                    shared.sleep = Some(sleep);
                    return Poll::Pending;
                }
            }

            assert!(buf.remaining() > 1); // TODO
            let wanted = buf.remaining() / WORD_SIZE as usize;
            let count = min(max_samples, wanted);

            let mut samples: Vec<i16> = Vec::with_capacity(count);
            let mut max_taken = true;

            for shared_input in shared.buffers.values_mut() {
                let remaining_samples = shared_input.buffer.remaining() / WORD_SIZE as usize;
                let to_read = min(count, remaining_samples);

                if remaining_samples > max_samples {
                    max_taken = false;
                }

                for i in 0..to_read {
                    let s = shared_input.buffer.get_i16_le();
                    if samples.len() > i {
                        samples[i] = samples[i].saturating_add(s);
                    } else {
                        samples.push(s);
                    }
                }

                if let Some(w) = shared_input.write_notify.take() {
                    w.wake();
                }
            }

            if samples.len() > 0 {
                buf.put_slice(slice_to_u8(&samples));

                if max_taken {
                    shared.last_read = now;
                } else {
                    shared.last_read = shared.last_read.add(Duration::from_millis(
                        samples.len() as u64 * 1000 / (FREQUENCY * CHANNELS) as u64,
                    ));
                    assert!(shared.last_read <= now);
                }

                Poll::Ready(Ok(()))
            } else {
                shared.read_notify = Some(cx.waker().clone());

                Poll::Pending
            }
        }
    }
}

impl Shared {
    fn cleanup(&mut self) {
        // Remove closed inputs where the buffer is empty
        let vec = self
            .buffers
            .iter()
            .filter(|(_, v)| v.closed && v.buffer.is_empty())
            .map(|(k, _)| *k)
            .collect::<Vec<_>>();

        for entry in vec {
            debug!("collected input {}", entry);
            self.buffers.remove(&entry);
        }
    }
}
