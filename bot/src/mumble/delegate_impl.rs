use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures::{Sink, SinkExt};
use mumble_protocol::voice::VoicePacket;
use mumble_protocol::Serverbound;

use crate::mumble::{AudioSink, AudioState, WithAddress};

impl Sink<VoicePacket<Serverbound>> for AudioState {
    type Error = <AudioSink as Sink<VoicePacket<Serverbound>>>::Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.sink.poll_ready_unpin(cx)
    }

    fn start_send(
        mut self: Pin<&mut Self>,
        item: VoicePacket<Serverbound>,
    ) -> Result<(), Self::Error> {
        self.sink.start_send_unpin(item)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.sink.poll_flush_unpin(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.sink.poll_close_unpin(cx)
    }
}

impl<T, I> Sink<I> for WithAddress<T>
where
    T: Sink<(I, SocketAddr)> + Unpin,
{
    type Error = T::Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.sink.poll_ready_unpin(cx)
    }

    fn start_send(mut self: Pin<&mut Self>, item: I) -> Result<(), Self::Error> {
        let addr = self.addr.clone();
        self.sink.start_send_unpin((item, addr))
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.sink.poll_flush_unpin(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.sink.poll_close_unpin(cx)
    }
}
