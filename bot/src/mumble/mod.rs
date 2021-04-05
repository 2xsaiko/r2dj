use std::sync::Arc;

use futures::stream::StreamExt;
use futures::SinkExt;
use log::info;
use mumble_protocol::control::{msgs, ClientControlCodec};
use mumble_protocol::crypt::ClientCryptState;
use sysinfo::SystemExt;
use tokio::net::UdpSocket;
use tokio::sync::{broadcast, mpsc, watch, Mutex};
use tokio::task::JoinHandle;
use tokio_util::codec::Decoder;
use tokio_util::udp::UdpFramed;

use crate::mixer::{new_mixer, MixerInput};
use crate::mumble::connect::{HandshakeState, ResultAction};
pub use crate::mumble::event::Event;
use crate::mumble::server_state::{ChannelRef, ServerState, UserRef};
use crate::mumble::state::ClientData;
use crate::{CRATE_NAME, CRATE_VERSION};

mod connect;
mod event;
mod server_state;
mod state;
mod tasks;

#[derive(Debug, Clone)]
pub struct MumbleConfig {
    pub username: String,
}

pub struct MumbleClient {
    stop_notify: watch::Sender<()>,
    tasks: Vec<JoinHandle<()>>,
    client_data: ClientData,
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

        sink.send(get_version_packet().into()).await.unwrap();

        let mut msg = msgs::Authenticate::new();
        msg.set_username(config.username);
        msg.set_opus(true);
        sink.send(msg.into()).await.unwrap();

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
            None => return Err(()),
            Some(cs) => cs,
        };

        let (audio_sink, audio_source) = UdpFramed::new(udp_socket, cs).split();

        let (voice_tx, voice_rx) = mpsc::channel(20);
        let (event_chan, _) = broadcast::channel(20);
        let (tcp_tx, tcp_rx) = mpsc::channel(20);
        let (udp_tx, udp_rx) = mpsc::channel(20);

        let (m_in, m_out) = new_mixer();

        let client_data = ClientData {
            server_state,
            session: UserRef::new(session_id),
            event_chan,
            mixer: m_in,
            voice_tx,
            tcp_tx,
            udp_tx,
        };

        let tasks = vec![
            tokio::spawn(tasks::main_task(
                client_data.clone(),
                source,
                audio_source,
                voice_rx,
                stop_rx,
            )),
            tokio::spawn(tasks::tcp_sender(tcp_rx, sink)),
            tokio::spawn(tasks::udp_sender(udp_rx, audio_sink, peer_addr)),
            tokio::spawn(tasks::encoder(client_data.clone(), m_out)),
        ];

        Ok(MumbleClient {
            stop_notify,
            tasks,
            client_data,
        })
    }

    pub fn event_listener(&self) -> broadcast::Receiver<Event> {
        self.client_data.event_chan.subscribe()
    }

    pub async fn send_channel_message(&self, text: &str) {
        let channel = self.channel().await;
        let mut m = msgs::TextMessage::new();
        m.mut_channel_id().push(channel.id());
        m.set_message(text.to_string());
        self.client_data.tcp_tx.send(m.into()).await.unwrap();
    }

    pub fn user(&self) -> UserRef {
        self.client_data.session
    }

    pub async fn channel(&self) -> ChannelRef {
        let lock = self.client_data.server_state.lock().await;

        let user = self.client_data.session.get(&lock).unwrap();
        user.channel()
    }

    pub fn server_state(&self) -> Arc<Mutex<ServerState>> {
        self.client_data.server_state.clone()
    }

    pub fn audio_input(&self) -> MixerInput {
        self.client_data.mixer.clone()
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

fn get_version_packet() -> msgs::Version {
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
    msg
}
