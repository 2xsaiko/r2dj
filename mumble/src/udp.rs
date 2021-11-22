use std::fmt::{Debug, Formatter};
use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use async_std::net::UdpSocket;
use asynchronous_codec::{Decoder, Encoder};
use bytes::{BufMut, BytesMut};
use futures::future::BoxFuture;
use futures::ready;
use futures::FutureExt;
use futures::Sink;
use futures::Stream;

/// A unified [`Stream`] and [`Sink`] interface to an underlying `UdpSocket`, using
/// the `Encoder` and `Decoder` traits to encode and decode frames.
///
/// Raw UDP sockets work with datagrams, but higher-level code usually wants to
/// batch these into meaningful chunks, called "frames". This method layers
/// framing on top of this socket by using the `Encoder` and `Decoder` traits to
/// handle encoding and decoding of messages frames. Note that the incoming and
/// outgoing frame types may be distinct.
///
/// This function returns a *single* object that is both [`Stream`] and [`Sink`];
/// grouping this into a single object is often useful for layering things which
/// require both read and write access to the underlying object.
///
/// If you want to work more directly with the streams and sink, consider
/// calling [`split`] on the `UdpFramed` returned by this method, which will break
/// them into separate objects, allowing them to interact more easily.
///
/// [`Stream`]: futures_core::Stream
/// [`Sink`]: futures_sink::Sink
/// [`split`]: https://docs.rs/futures/0.3/futures/stream/trait.StreamExt.html#method.split
#[must_use = "sinks do nothing unless polled"]
#[derive(Debug)]
pub struct UdpFramed<C> {
    socket: Arc<UdpSocket>,
    codec: C,
    rd: PollState<(usize, SocketAddr)>,
    wr: PollState<usize>,
    out_addr: SocketAddr,
    flushed: bool,
    is_readable: bool,
    current_addr: Option<SocketAddr>,
}

enum PollState<R> {
    Idle(BytesMut),
    Pending(BoxFuture<'static, (BytesMut, io::Result<R>)>),
    Invalid,
}

impl<R> Debug for PollState<R> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            PollState::Idle(v) => f.debug_tuple("Idle").field(v).finish(),
            PollState::Pending(_) => f.debug_tuple("Pending").finish(),
            PollState::Invalid => unreachable!(),
        }
    }
}

const INITIAL_RD_CAPACITY: usize = 64 * 1024;
const INITIAL_WR_CAPACITY: usize = 8 * 1024;

impl<C: Decoder + Unpin> Stream for UdpFramed<C> {
    type Item = Result<(C::Item, SocketAddr), C::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let pin = self.get_mut();

        if let PollState::Idle(rd) = &mut pin.rd {
            rd.reserve(INITIAL_RD_CAPACITY);
        }

        loop {
            // Are there still bytes left in the read buffer to decode?
            if let PollState::Idle(rd) = &mut pin.rd {
                if pin.is_readable {
                    if let Some(frame) = pin.codec.decode_eof(rd)? {
                        let current_addr = pin
                            .current_addr
                            .expect("will always be set before this line is called");

                        return Poll::Ready(Some(Ok((frame, current_addr))));
                    }

                    // if this line has been reached then decode has returned `None`.
                    pin.is_readable = false;
                    rd.clear();
                }
            }

            // We're out of data. Try and fetch more data to decode
            let addr = unsafe {
                let socket = pin.socket.clone();

                let mut fut = match std::mem::replace(&mut pin.rd, PollState::Invalid) {
                    PollState::Idle(mut rd) => {
                        let fut = async move {
                            unsafe fn conv(slice: &mut bytes::buf::UninitSlice) -> &mut [u8] {
                                &mut *(slice as *mut _ as *mut [u8])
                            }

                            // Convert `&mut [MaybeUnit<u8>]` to `&mut [u8]` because we will be
                            // writing to it via `recv_from` and therefore initializing the memory.
                            let mut buf = conv(rd.chunk_mut());
                            let res = socket.recv_from(&mut buf).await;
                            (rd, res)
                        }
                        .boxed();
                        fut
                    }
                    PollState::Pending(fut) => fut,
                    PollState::Invalid => unreachable!(),
                };

                let (mut buf, res) = match fut.poll_unpin(cx) {
                    Poll::Ready(v) => v,
                    Poll::Pending => {
                        pin.rd = PollState::Pending(fut);
                        return Poll::Pending;
                    }
                };

                let (len, addr) = res?;
                buf.advance_mut(len);
                pin.rd = PollState::Idle(buf);
                addr
            };

            pin.current_addr = Some(addr);
            pin.is_readable = true;
        }
    }
}

impl<I, C: Encoder<Item = I> + Unpin> Sink<(I, SocketAddr)> for UdpFramed<C> {
    type Error = C::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if !self.flushed {
            match self.poll_flush(cx)? {
                Poll::Ready(()) => {}
                Poll::Pending => return Poll::Pending,
            }
        }

        Poll::Ready(Ok(()))
    }

    fn start_send(self: Pin<&mut Self>, item: (I, SocketAddr)) -> Result<(), Self::Error> {
        let (frame, out_addr) = item;

        let pin = self.get_mut();

        let mut wr = match std::mem::replace(&mut pin.wr, PollState::Invalid) {
            PollState::Idle(wr) => wr,
            PollState::Pending(_) => panic!("called start_send while send already in progress"),
            PollState::Invalid => unreachable!(),
        };

        match pin.codec.encode(frame, &mut wr) {
            Ok(_) => {}
            Err(e) => {
                pin.rd = PollState::Idle(wr);
                return Err(e);
            }
        }

        pin.out_addr = out_addr;
        pin.flushed = false;

        let socket = pin.socket.clone();
        pin.wr = PollState::Pending(
            async move {
                let res = socket.send_to(&wr, out_addr).await;

                (wr, res)
            }
            .boxed(),
        );

        Ok(())
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if self.flushed {
            return Poll::Ready(Ok(()));
        }

        let Self {
            ref mut socket,
            ref mut out_addr,
            ref mut wr,
            ..
        } = *self;

        let (mut wr, res) = match std::mem::replace(&mut self.wr, PollState::Invalid) {
            PollState::Idle(buf) => {
                self.wr = PollState::Idle(buf);
                return Poll::Ready(Ok(()));
            }
            PollState::Pending(mut fut) => {
                match fut.poll_unpin(cx) {
                    Poll::Ready(v) => v,
                    Poll::Pending => {
                        self.wr = PollState::Pending(fut);
                        return Poll::Pending;
                    }
                }
            }
            PollState::Invalid => unreachable!(),
        };

        let n = match res {
            Ok(v) => v,
            Err(e) => {
                self.wr = PollState::Idle(wr);
                return Poll::Ready(Err(e.into()));
            }
        };

        let wrote_all = n == wr.len();
        wr.clear();
        self.flushed = true;

        self.wr = PollState::Idle(wr);

        let res = if wrote_all {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::Other,
                "failed to write entire datagram to socket",
            )
            .into())
        };

        Poll::Ready(res)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        ready!(self.poll_flush(cx))?;
        Poll::Ready(Ok(()))
    }
}

impl<C> UdpFramed<C> {
    /// Create a new `UdpFramed` backed by the given socket and codec.
    ///
    /// See struct level documentation for more details.
    pub fn new(socket: UdpSocket, codec: C) -> UdpFramed<C> {
        Self {
            socket: Arc::new(socket),
            codec,
            out_addr: SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), 0)),
            rd: PollState::Idle(BytesMut::with_capacity(INITIAL_RD_CAPACITY)),
            wr: PollState::Idle(BytesMut::with_capacity(INITIAL_WR_CAPACITY)),
            flushed: true,
            is_readable: false,
            current_addr: None,
        }
    }

    /// Returns a reference to the underlying I/O stream wrapped by `Framed`.
    ///
    /// # Note
    ///
    /// Care should be taken to not tamper with the underlying stream of data
    /// coming in as it may corrupt the stream of frames otherwise being worked
    /// with.
    pub fn get_ref(&self) -> &UdpSocket {
        &self.socket
    }

    /// Consumes the `Framed`, returning its underlying I/O stream.
    pub fn into_inner(self) -> UdpSocket {
        // these can have references to the inner Arc, so drop them first so the
        // try_unwrap succeeds... hopefully
        drop(self.rd);
        drop(self.wr);

        Arc::try_unwrap(self.socket).expect("extra reference to UdpSocket somewhere?")
    }

    /// Returns a reference to the underlying codec wrapped by
    /// `Framed`.
    ///
    /// Note that care should be taken to not tamper with the underlying codec
    /// as it may corrupt the stream of frames otherwise being worked with.
    pub fn codec(&self) -> &C {
        &self.codec
    }

    /// Returns a mutable reference to the underlying codec wrapped by
    /// `UdpFramed`.
    ///
    /// Note that care should be taken to not tamper with the underlying codec
    /// as it may corrupt the stream of frames otherwise being worked with.
    pub fn codec_mut(&mut self) -> &mut C {
        &mut self.codec
    }

    /// Returns a reference to the read buffer.
    pub fn read_buffer(&self) -> Option<&BytesMut> {
        match &self.rd {
            PollState::Idle(rd) => Some(rd),
            _ => None,
        }
    }

    /// Returns a mutable reference to the read buffer.
    pub fn read_buffer_mut(&mut self) -> Option<&mut BytesMut> {
        match &mut self.rd {
            PollState::Idle(rd) => Some(rd),
            _ => None,
        }
    }
}
