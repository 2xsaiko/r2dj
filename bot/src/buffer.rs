use std::cmp::min;
use std::io;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use bytes::{Buf, BytesMut};
use futures::task::{Context, Poll, Waker};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

struct Shared {
    write_notify: Option<Waker>,
    read_notify: Option<Waker>,
    buffer: BytesMut,
    buf_size: usize,
    closed: bool,
}

pub struct BufferInput {
    shared: Arc<Mutex<Shared>>,
}

pub struct BufferOutput {
    shared: Arc<Mutex<Shared>>,
}

pub fn new_buffer(buf_size: usize) -> (BufferInput, BufferOutput) {
    let shared = Arc::new(Mutex::new(Shared {
        write_notify: None,
        read_notify: None,
        buffer: BytesMut::with_capacity(buf_size),
        buf_size,
        closed: false,
    }));

    (
        BufferInput {
            shared: shared.clone(),
        },
        BufferOutput { shared },
    )
}

impl AsyncWrite for BufferInput {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let mut shared = self.shared.lock().unwrap();

        if shared.buffer.len() < shared.buf_size {
            let to_write = min(buf.len(), shared.buf_size - shared.buffer.len());
            shared.buffer.extend_from_slice(&buf[..to_write]);

            if let Some(w) = shared.read_notify.take() {
                w.wake();
            }

            Poll::Ready(Ok(to_write))
        } else {
            shared.write_notify = Some(cx.waker().clone());
            Poll::Pending
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        let mut shared = self.shared.lock().unwrap();
        shared.closed = true;

        Poll::Ready(Ok(()))
    }
}

impl Drop for BufferInput {
    fn drop(&mut self) {
        let mut shared = self.shared.lock().unwrap();
        shared.closed = true;
    }
}

impl AsyncRead for BufferOutput {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let mut shared = self.shared.lock().unwrap();

        if !shared.buffer.is_empty() {
            let to_read = min(buf.remaining(), shared.buffer.len());
            let mut b = vec![0; to_read];
            shared.buffer.copy_to_slice(&mut b);
            buf.put_slice(&b);

            if let Some(w) = shared.write_notify.take() {
                w.wake();
            }

            Poll::Ready(Ok(()))
        } else if shared.closed {
            // EOF
            Poll::Ready(Ok(()))
        } else {
            shared.read_notify = Some(cx.waker().clone());

            Poll::Pending
        }
    }
}
