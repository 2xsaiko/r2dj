use std::net::SocketAddr;
use std::sync::Arc;

use audiopus::{Application, Channels, SampleRate};
use bytes::Bytes;
use futures::stream::{SplitSink, SplitStream, StreamExt};
use futures::SinkExt;
use log::info;
use mumble_protocol::control::{msgs, ClientControlCodec, ControlPacket};
use mumble_protocol::crypt::ClientCryptState;
use mumble_protocol::voice::{VoicePacket, VoicePacketPayload};
use mumble_protocol::Serverbound;
use sysinfo::SystemExt;
use tokio::io;
use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt;
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::{watch, Mutex, broadcast};
use tokio::task::JoinHandle;
use tokio::time;
use tokio::time::Duration;
use tokio_rustls::client::TlsStream;
use tokio_util::codec::{Decoder, Framed};
use tokio_util::udp::UdpFramed;

use crate::mumble::connect::{HandshakeState, ResultAction};
use crate::mumble::server_state::{ServerState, UserRef, ChannelRef};
use crate::util::slice_to_u8_mut;
use crate::{CRATE_NAME, CRATE_VERSION};

mod connect;
mod delegate_impl;
mod logic;
mod server_state;
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
    stop_notify: watch::Sender<()>,
    tasks: Vec<JoinHandle<()>>,
    audio_state: Arc<Mutex<AudioState>>,
    event_chan: broadcast::Sender<Event>,
    tcp_sink: Arc<Mutex<PacketSink>>,
    server_state: Arc<Mutex<ServerState>>,
    session_id: u32,
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

        let (stop_notify, stop_rx) = watch::channel(());

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
        let (tx, _) = broadcast::channel(20);
        let server_state = Arc::new(Mutex::new(ServerState::new(tx.clone())));

        let result: Option<(ClientCryptState, u32)> = loop {
            match source.next().await {
                None => break None,
                Some(packet) => {
                    let packet = packet.unwrap();

                    match connect::handle_packet(handshake_state, &server_state, packet).await {
                        ResultAction::Continue(state) => handshake_state = state,
                        ResultAction::Disconnect => break None,
                        ResultAction::TransferConnected(a, s) => break Some((a, s)),
                    }
                }
            }
        };

        let (cs, session_id) = match result {
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


        let tcp_handler = tokio::spawn(session::tcp_handler(
            stop_rx.clone(),
            source,
            sink.clone(),
            server_state.clone(),
            tx.clone(),
        ));
        let udp_handler = tokio::spawn(session::udp_handler(
            stop_rx.clone(),
            audio_source,
            audio_state.clone(),
        ));

        let tasks = vec![tcp_handler, udp_handler];

        Ok(MumbleClient {
            stop_notify,
            tasks,
            audio_state,
            event_chan: tx,
            tcp_sink: sink,
            server_state,
            session_id,
        })
    }

    pub fn event_listener(&self) -> broadcast::Receiver<Event> {
        self.event_chan.subscribe()
    }

    pub async fn send_channel_message(&self, text: &str) {
        let channel = self.channel().await;
        let mut lock = self.tcp_sink.lock().await;
        let mut m = msgs::TextMessage::new();
        m.mut_channel_id().push(channel.id());
        m.set_message(text.to_string());
        lock.send(m.into()).await.unwrap();
    }

    pub fn user(&self) -> UserRef {
        UserRef::new(self.session_id)
    }

    pub async fn channel(&self) -> ChannelRef {
        let lock = self.server_state.lock().await;

        let user = lock.user(self.session_id).unwrap();
        user.channel()
    }

    pub fn server_state(&self) -> Arc<Mutex<ServerState>> {
        self.server_state.clone()
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
        let _ = self.stop_notify.send(());

        for fut in self.tasks.drain(..) {
            fut.await.unwrap();
        }
    }
}

impl Drop for MumbleClient {
    fn drop(&mut self) {
        let _ = self.stop_notify.send(());
    }
}

pub struct WithAddress<T> {
    addr: SocketAddr,
    sink: T,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Event {
    Message {
        actor: Option<UserRef>,
        receivers: Vec<UserRef>,
        channels: Vec<ChannelRef>,
        message: String,
    },
    UserMoved {
        user: UserRef,
        old_channel: ChannelRef,
        new_channel: ChannelRef,
    }
}
