use std::sync::Arc;

use futures::stream::StreamExt;
use futures::SinkExt;
use log::info;
use mumble_protocol::control::{msgs, ClientControlCodec};
use mumble_protocol::crypt::ClientCryptState;
use sysinfo::SystemExt;
use tokio::net::UdpSocket;
use tokio::sync::{broadcast, watch, Mutex};
use tokio::task::JoinHandle;
use tokio_util::codec::Decoder;
use tokio_util::udp::UdpFramed;

use audiopipe::mixer::MixerInput;

use crate::connect::{HandshakeState, ResultAction};
pub use crate::event::Event;
use crate::server_state::{ChannelRef, ServerState, UserRef};
use crate::tasks::{ConnectionInfo, Connectors};

mod connect;
mod event;
mod server_state;
mod tasks;

const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone)]
pub struct MumbleConfig {
    pub username: String,
}

pub struct MumbleClient {
    stop_notify: watch::Sender<()>,
    tasks: Vec<JoinHandle<()>>,
    connectors: Connectors,
    session: UserRef,
    server_state: Arc<Mutex<ServerState>>,
}

impl MumbleClient {
    pub async fn connect(host: &str, port: u16, config: MumbleConfig) -> Result<Self, ()> {
        let (stop_notify, stop_rx) = watch::channel(());
        let connectors = Connectors::new(stop_rx);

        // actually connect

        info!("Connecting to {}, port {}", host, port);
        let stream = connect::connect(host, port)
            .await
            .expect("failed to connect to server");

        let peer_addr = stream.get_ref().0.peer_addr().unwrap();

        let mut tcp = ClientControlCodec::new().framed(stream);

        tcp.send(get_version_packet().into()).await.unwrap();

        let mut msg = msgs::Authenticate::new();
        msg.set_username(config.username);
        msg.set_opus(true);
        tcp.send(msg.into()).await.unwrap();

        let mut handshake_state = HandshakeState::default();
        let (tx, _) = broadcast::channel(20);
        let server_state = Arc::new(Mutex::new(ServerState::new(tx.clone())));

        let result: Option<(ClientCryptState, u32)> = loop {
            match tcp.next().await {
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

        let udp_socket = UdpSocket::bind(tcp.get_ref().get_ref().0.local_addr().unwrap())
            .await
            .expect("failed to open UDP socket");
        let udp = UdpFramed::new(udp_socket, cs);

        let connection_info = ConnectionInfo::new(tcp, udp, peer_addr, server_state.clone());

        let tasks = tasks::start_tasks(connection_info, connectors.clone()).await;

        Ok(MumbleClient {
            stop_notify,
            tasks,
            connectors,
            session: UserRef::new(session_id),
            server_state,
        })
    }

    pub fn event_subscriber(&self) -> broadcast::Receiver<Event> {
        self.connectors.event_subscriber()
    }

    pub async fn send_channel_message(&self, text: &str) {
        let channel = self.channel().await;
        let mut m = msgs::TextMessage::new();
        m.mut_channel_id().push(channel.id());
        m.set_message(text.to_string());
        self.connectors.cp_tx().send(m.into()).await.unwrap();
    }

    pub async fn set_comment<S>(&self, text: S)
    where
        S: Into<String>,
    {
        let mut state = msgs::UserState::new();
        state.set_comment(text.into());
        self.connectors.cp_tx().send(state.into()).await.unwrap();
    }

    pub fn user(&self) -> UserRef {
        self.session
    }

    pub async fn channel(&self) -> ChannelRef {
        let lock = self.server_state.lock().await;

        let user = self.session.get(&lock).unwrap();
        user.channel()
    }

    pub fn server_state(&self) -> Arc<Mutex<ServerState>> {
        self.server_state.clone()
    }

    pub fn audio_input(&self) -> MixerInput {
        self.connectors.audio_input()
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
    msg.set_release(format!("{} {}", "mumble-rs", CRATE_VERSION));
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
