use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use pin_project_lite::pin_project;

use crate::streamio::StreamWrite;

pin_project! {
    pub struct Map<F, T> {
        op: F,
        #[pin]
        stream: T,
    }
}

impl<F, T> Map<F, T> {
    pub fn new(op: F, stream: T) -> Self {
        Map { op, stream }
    }
}

impl<F, T, I, O> StreamWrite<I> for Map<F, T>
where
    F: Fn(&I) -> O,
    T: StreamWrite<O> + Unpin,
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[I],
    ) -> Poll<io::Result<usize>> {
        let mut buffer = Vec::with_capacity(buf.len());

        for entry in buf {
            let entry_transformed = (self.op)(entry);
            buffer.push(entry_transformed);
        }

        self.project().stream.poll_write(cx, &buffer)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.project().stream.poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.project().stream.poll_close(cx)
    }
}
