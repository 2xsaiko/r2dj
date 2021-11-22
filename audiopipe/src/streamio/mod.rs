use std::future::Future;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use map::Map;

use crate::streamio::read_buf::ReadBuf;

pub mod map;
pub mod read_buf;

pub trait StreamWrite<T> {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[T])
        -> Poll<io::Result<usize>>;

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>>;

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>>;
}

impl<T, U> StreamWrite<T> for &mut U
where
    U: StreamWrite<T> + Unpin + ?Sized,
{
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[T],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut **self).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut **self).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut **self).poll_close(cx)
    }
}

pub trait StreamRead<T> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_, T>,
    ) -> Poll<io::Result<()>>;
}

pub trait StreamWriteExt<T>: StreamWrite<T> {
    fn write<'a>(&'a mut self, _buf: &'a [T]) -> Write<'a, Self, T> {
        todo!()
    }

    fn map<F, U>(self, op: F) -> Map<F, Self>
    where
        F: Fn(&U) -> T,
        Self: Sized,
    {
        Map::new(op, self)
    }
}

pub struct Write<'a, S: ?Sized, T> {
    pipe: &'a mut S,
    buf: &'a [T],
}

impl<'a, S, T> Future for Write<'a, S, T>
where
    S: StreamWrite<T> + Unpin,
{
    type Output = io::Result<usize>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;
        StreamWrite::poll_write(Pin::new(this.pipe), cx, this.buf)
    }
}
