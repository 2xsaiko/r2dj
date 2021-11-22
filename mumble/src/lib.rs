#![feature(try_trait_v2)]

use std::path::Path;

use async_broadcast as broadcast;
use async_std::net::UdpSocket;
use asynchronous_codec::Framed;
use futures::stream::StreamExt;
use futures::SinkExt;
use log::info;
use mumble_protocol::control::{msgs, ClientControlCodec};
use mumble_protocol::crypt::ClientCryptState;
use petgraph::graph::NodeIndex;
use sysinfo::SystemExt;

use audiopipe::Core;
use msgtools::{proxy, Ac};
use udp::UdpFramed;

use crate::connect::{HandshakeState, ResultAction};
pub use crate::event::Event;
use crate::server_state::{Channel, ChannelRef, ServerState, User, UserRef};

mod connect;
pub mod event;
mod server_state;
mod tasks;
mod udp;

const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone)]
pub struct MumbleConfig {
    pub username: String,
}

proxy! {
    pub proxy MumbleClient {
        pub async fn broadcast_message(channels: Vec<ChannelRef>, users: Vec<UserRef>, text: String);
        pub async fn set_comment(comment: String);
        pub async fn my_user() -> Ac<User>;
        pub async fn my_user_ref() -> UserRef;
        pub async fn my_channel() -> Ac<Channel>;
        pub async fn my_channel_ref() -> ChannelRef;
        pub async fn get_user(r: UserRef) -> Option<Ac<User>>;
        pub async fn state() -> Ac<ServerState>;
        pub async fn max_message_length() -> Option<u32>;
        pub async fn allow_html_messages() -> Option<bool>;
        pub async fn audio_input() -> NodeIndex;
        pub async fn event_subscriber() -> broadcast::Receiver<Event>;
        pub async fn close();
    }
}

impl MumbleClient {
    pub async fn connect(
        host: &str,
        port: u16,
        certfile: Option<impl AsRef<Path>>,
        config: MumbleConfig,
        ac: &Core,
    ) -> Result<Self, ()> {
        info!("Connecting to {}, port {}", host, port);

        if let Some(certfile) = &certfile {
            info!("Using certificate '{}'", certfile.as_ref().display());
        }

        let stream = connect::connect(host, port, certfile)
            .await
            .expect("failed to connect to server");

        let peer_addr = stream.get_ref().peer_addr().unwrap();
        let local_addr = stream.get_ref().local_addr().unwrap();

        let mut tcp = Framed::new(stream, ClientControlCodec::new());

        tcp.send(get_version_packet().into()).await.unwrap();

        let mut msg = msgs::Authenticate::new();
        msg.set_username(config.username);
        msg.set_opus(true);
        tcp.send(msg.into()).await.unwrap();

        let mut handshake_state = HandshakeState::default();
        let (tx, rx) = broadcast::broadcast(20);
        let mut server_state = Ac::new(ServerState::new(tx.clone()));

        let result: Option<(ClientCryptState, u32)> = loop {
            match tcp.next().await {
                None => break None,
                Some(packet) => {
                    let packet = packet.unwrap();

                    match connect::handle_packet(handshake_state, &mut server_state, packet).await {
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

        let udp_socket = UdpSocket::bind(local_addr)
            .await
            .expect("failed to open UDP socket");
        let udp = UdpFramed::new(udp_socket, cs);

        let (client, recv) = MumbleClient::channel();

        let state = tasks::State::new(
            recv,
            tcp,
            udp,
            peer_addr,
            ac.add_output(),
            server_state,
            UserRef::new(session_id),
        );
        async_std::task::spawn(state.handle_messages());

        Ok(client)
    }

    pub async fn message_my_channel(&self, text: &str) -> proxy::Result {
        self.message_channel(self.my_channel_ref().await?, text)
            .await
    }

    pub async fn message_channel<S>(&self, channel: ChannelRef, text: S) -> proxy::Result
    where
        S: Into<String>,
    {
        self.broadcast_message(vec![channel], vec![], text.into())
            .await
    }

    pub async fn message_user<S>(&self, user: UserRef, text: S) -> proxy::Result
    where
        S: Into<String>,
    {
        self.broadcast_message(vec![], vec![user], text.into())
            .await
    }

    pub async fn respond<S>(&self, ev: &event::Message, text: S) -> proxy::Result
    where
        S: Into<String>,
    {
        let mut users = ev.receivers.clone();

        if let Some(actor) = ev.actor {
            users.push(actor);
        }

        self.broadcast_message(
            ev.channels.iter().cloned().collect(),
            users.into_iter().collect(),
            text.into(),
        )
        .await
    }
}

fn get_version_packet() -> msgs::Version {
    let mut msg = msgs::Version::new();
    msg.set_version(0x00010204);
    msg.set_release(format!("{} {}", "mumble-rs", CRATE_VERSION));
    let info = sysinfo::System::new();
    msg.set_os(info.name().unwrap_or_else(|| "unknown".to_string()));
    msg.set_os_version(format!(
        "{}; {}",
        info.os_version().unwrap_or_else(|| "unknown".to_string()),
        info.kernel_version()
            .unwrap_or_else(|| "unknown".to_string())
    ));
    msg
}
