use std::io::Error;
use std::net::SocketAddr;
use std::ops::Add;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use audiopus::coder::Encoder;
use audiopus::{Application, Channels, SampleRate};
use bytes::Bytes;
use futures::stream::{SplitSink, SplitStream, StreamExt};
use futures::task::{Context, Poll};
use futures::{Sink, SinkExt};
use log::{debug, info};
use mumble_protocol::control::{msgs, ClientControlCodec, ControlPacket};
use mumble_protocol::crypt::ClientCryptState;
use mumble_protocol::voice::{VoicePacket, VoicePacketPayload};
use mumble_protocol::Serverbound;
use sysinfo::SystemExt;
use tokio::io;
use tokio::io::AsyncReadExt;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio::time;
use tokio::time::{timeout_at, Duration, Instant};
use tokio_rustls::client::TlsStream;
use tokio_util::codec::{Decoder, Framed};
use tokio_util::udp::UdpFramed;

use crate::mumble::connect::{HandshakeState, ResultAction};
use crate::{CRATE_NAME, CRATE_VERSION};
use crate::util::slice_to_u8_mut;

mod connect;
mod delegate_impl;
mod logic;
mod session;

#[derive(Debug, Clone)]
pub struct MumbleConfig {
    pub username: String,
}

type PacketSink =
    SplitSink<Framed<TlsStream<TcpStream>, ClientControlCodec>, ControlPacket<Serverbound>>;
type PacketSource = SplitStream<Framed<TlsStream<TcpStream>, ClientControlCodec>>;
type AudioSink =
    WithAddress<SplitSink<UdpFramed<ClientCryptState>, (VoicePacket<Serverbound>, SocketAddr)>>;
type AudioSource = SplitStream<UdpFramed<ClientCryptState>>;

pub struct MumbleClient {
    running: Arc<AtomicBool>,
    tasks: Vec<JoinHandle<()>>,
    sink: Arc<Mutex<PacketSink>>,
    audio_state: Arc<Mutex<AudioState>>,
}

pub struct AudioState {
    sink: AudioSink,
    seq: u64,
}

impl MumbleClient {
    pub async fn connect(host: &str, port: u16, config: MumbleConfig) -> Result<Self, ()> {
        info!("Connecting to {}, port {}", host, port);
        let stream = connect::connect(host, port)
            .await
            .expect("failed to connect to server");

        let udp_socket = UdpSocket::bind(stream.get_ref().0.local_addr().unwrap())
            .await
            .expect("failed to open UDP socket");
        let peer_addr = stream.get_ref().0.peer_addr().unwrap();

        let running = Arc::new(AtomicBool::new(true));

        let (mut sink, mut source) = ClientControlCodec::new().framed(stream).split();

        let mut msg = msgs::Version::new();
        msg.set_version(0x00010204);
        msg.set_release(format!("{} {}", CRATE_NAME, CRATE_VERSION));
        let info = sysinfo::System::new();
        msg.set_os(info.get_name().unwrap_or_else(|| "unknown".to_string()));
        msg.set_os_version(format!(
            "{}; {}",
            info.get_os_version()
                .unwrap_or_else(|| "unknown".to_string()),
            info.get_kernel_version()
                .unwrap_or_else(|| "unknown".to_string())
        ));
        sink.send(msg.into()).await.unwrap();

        let mut msg = msgs::Authenticate::new();
        msg.set_username(config.username);
        msg.set_opus(true);
        sink.send(msg.into()).await.unwrap();

        let sink = Arc::new(Mutex::new(sink));

        let mut handshake_state = HandshakeState::default();

        let cs: Option<ClientCryptState> = loop {
            match source.next().await {
                None => break None,
                Some(packet) => {
                    let packet = packet.unwrap();

                    match connect::handle_packet(handshake_state, packet).await {
                        ResultAction::Continue(state) => handshake_state = state,
                        ResultAction::Disconnect => break None,
                        ResultAction::TransferConnected(a) => break Some(a),
                    }
                }
            }
        };

        let cs = match cs {
            None => {
                return Err(());
            }
            Some(cs) => cs,
        };

        let (audio_sink, audio_source) = UdpFramed::new(udp_socket, cs).split();

        let udp_sink = WithAddress {
            addr: peer_addr,
            sink: audio_sink,
        };

        let audio_state = Arc::new(Mutex::new(AudioState {
            sink: udp_sink,
            seq: 0,
        }));

        let tcp_keepalive = tokio::spawn(session::tcp_keepalive(running.clone(), sink.clone()));
        let tcp_handler = tokio::spawn(session::tcp_handler(running.clone(), source));
        let udp_keepalive =
            tokio::spawn(session::udp_keepalive(running.clone(), audio_state.clone()));
        let udp_handler = tokio::spawn(session::udp_handler(running.clone(), audio_source));

        let tasks = vec![tcp_keepalive, tcp_handler, udp_keepalive, udp_handler];

        Ok(MumbleClient {
            running,
            tasks,
            sink,
            audio_state,
        })
    }

    pub async fn consume<T>(&self, mut pipe: T) -> io::Result<()>
    where
        T: AsyncRead + Unpin,
    {
        let ms_buf_size = 10;
        let sample_rate = SampleRate::Hz48000;
        let samples = sample_rate as usize * ms_buf_size / 1000;

        let bandwidth = 192000;
        let opus_buf_size = bandwidth / 8 * ms_buf_size / 1000;

        let mut pcm_buf = vec![0i16; samples];
        let mut opus_buf = vec![0u8; opus_buf_size];

        let encoder =
            audiopus::coder::Encoder::new(sample_rate, Channels::Mono, Application::Audio).unwrap();

        let mut interval = time::interval(Duration::from_millis(ms_buf_size as u64));

        let mut extra_byte = false;

        loop {
            interval.tick().await;

            let u8_buf = if extra_byte {
                &mut slice_to_u8_mut(&mut pcm_buf)[1..]
            } else {
                slice_to_u8_mut(&mut pcm_buf)
            };

            let r = pipe.read(u8_buf).await;

            if let Ok(mut r) = r {
                if r == 0 {
                    // self.send_audio_frame(VoicePacketPayload::Opus(Bytes::new(), true))
                    //     .await?;

                    break;
                }

                if extra_byte {
                    r += 1;
                }

                // divide by 2 since this is the size in bytes
                let input_len = r / 2;

                // adjust volume
                for el in pcm_buf[..input_len].iter_mut() {
                    *el = (*el as f32 * 0.1) as i16;
                }

                // OPUS does not like encoding less data, so let's fill the rest
                // with zeroes and send over the whole buffer >_>

                if input_len < pcm_buf.len() {
                    pcm_buf[input_len..].fill(0);
                }

                let len = encoder.encode(&pcm_buf, &mut opus_buf).unwrap();

                self.send_audio_frame(VoicePacketPayload::Opus(
                    Bytes::copy_from_slice(&opus_buf[..len]),
                    input_len < pcm_buf.len(),
                ))
                .await?;

                if r % 2 != 0 {
                    let u8_buf = slice_to_u8_mut(&mut pcm_buf);
                    u8_buf[0] = u8_buf[r - 1];
                    extra_byte = true;
                } else {
                    extra_byte = false;
                }
            }
        }

        Ok(())
    }

    pub async fn send_audio_frame(&self, payload: VoicePacketPayload) -> io::Result<()> {
        let mut sink = self.audio_state.lock().await;

        let packet = VoicePacket::Audio {
            _dst: Default::default(),
            target: 0,
            session_id: (),
            seq_num: sink.seq,
            payload,
            position_info: None,
        };

        sink.seq += 1;

        sink.send(packet).await
    }

    pub async fn close(mut self) {
        self.running.store(false, Ordering::Relaxed);

        for fut in self.tasks.drain(..) {
            fut.await.unwrap();
        }
    }
}

impl Drop for MumbleClient {
    fn drop(&mut self) {
        if self.running.load(Ordering::Relaxed) {
            debug!("Mumble client was dropped without closing it first!");
            self.running.store(false, Ordering::Relaxed);
        }
    }
}

pub struct WithAddress<T> {
    addr: SocketAddr,
    sink: T,
}

// pub struct AudioPipe {
//     state: Arc<Mutex<AudioState>>,
//     encoder: Encoder,
//     ms_buf_size: usize,
//     last_packet: Option<Instant>,
//     pcm_buf: Vec<i16>,
//     opus_buf_size: usize,
// }
//
// impl AudioPipe {
//     fn new(
//         client: &MumbleClient,
//         ms_buf_size: usize,
//         sample_rate: SampleRate,
//         bandwidth: usize,
//     ) -> Self {
//         let samples = sample_rate as usize * ms_buf_size / 1000;
//         let opus_buf_size = bandwidth / 8 * ms_buf_size / 1000;
//
//         let mut pcm_buf = vec![0; samples];
//
//         let encoder =
//             audiopus::coder::Encoder::new(sample_rate, Channels::Mono, Application::Audio).unwrap();
//
//         AudioPipe {
//             state: client.audio_state.clone(),
//             encoder,
//             ms_buf_size,
//             last_packet: None,
//             pcm_buf,
//             opus_buf_size,
//         }
//     }
// }
//
// impl AsyncWrite for AudioPipe {
//     fn poll_write(
//         self: Pin<&mut Self>,
//         cx: &mut Context<'_>,
//         buf: &[u8],
//     ) -> Poll<Result<usize, Error>> {
//         if let Some(lp) = self.last_packet {
//             if Instant::now().duration_since(lp).as_secs() > 2 {}
//         }
//
//         unimplemented!()
//     }
//
//     fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
//         unimplemented!()
//     }
//
//     fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
//         self.poll_flush(cx)
//     }
// }
