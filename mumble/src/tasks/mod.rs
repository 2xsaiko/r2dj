use std::fmt::Display;
use std::io;
use std::net::SocketAddr;
use std::ops::{ControlFlow, Try};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use async_broadcast as broadcast;
use async_std::stream::interval;
use async_std::sync::Mutex as AsyncMutex;
use futures::channel::mpsc;
use futures::select;
use futures::stream::Fuse;
use futures::{Sink, SinkExt, Stream, StreamExt};
use log::{debug, error};
use mumble_protocol::control::{msgs, ControlPacket};
use mumble_protocol::voice::VoicePacket;
use mumble_protocol::{Clientbound, Serverbound};
use petgraph::graph::NodeIndex;

use audiopipe::OutputSignal;
use encoder::encoder;
use msgtools::Ac;

use crate::event::{Event, Message};
use crate::server_state::{ChannelRef, ServerState, UserRef};
use crate::{MumbleClientMessage, MumbleClientReceiver};

mod encoder;

pub struct State<T, U> {
    pipe: MumbleClientReceiver,
    tcp: Fuse<T>,
    udp: Fuse<U>,
    peer: SocketAddr,
    server_state: Ac<ServerState>,
    event_chan: broadcast::Sender<Event>,
    event_sub: broadcast::InactiveReceiver<Event>,
    audio_seq: u64,
    output: Arc<AsyncMutex<OutputSignal>>,
    output_id: NodeIndex,
    me: UserRef,
}

impl<T, U> State<T, U>
where
    T: Stream,
    U: Stream,
{
    pub fn new(
        pipe: MumbleClientReceiver,
        tcp: T,
        udp: U,
        peer: SocketAddr,
        output: OutputSignal,
        server_state: Ac<ServerState>,
        me: UserRef,
    ) -> Self {
        let (event_chan, event_sub) = broadcast::broadcast(20);
        let event_sub = event_sub.deactivate();
        let output_id = output.node();
        let output = Arc::new(AsyncMutex::new(output));

        State {
            pipe,
            tcp: tcp.fuse(),
            udp: udp.fuse(),
            peer,
            server_state,
            event_chan,
            event_sub,
            audio_seq: 0,
            output,
            output_id,
            me,
        }
    }
}

macro_rules! try_or_break {
    ($e:expr) => {
        match Try::branch($e) {
            ControlFlow::Continue(v) => v,
            ControlFlow::Break(_) => break,
        }
    };
}

impl<T, U> State<T, U>
where
    T: Stream<Item = io::Result<ControlPacket<Clientbound>>>
        + Sink<ControlPacket<Serverbound>>
        + Unpin,
    T::Error: Display,
    U: Stream<Item = io::Result<(VoicePacket<Clientbound>, SocketAddr)>>
        + Sink<(VoicePacket<Serverbound>, SocketAddr)>
        + Unpin,
    U::Error: Display,
{
    pub async fn handle_messages(mut self) {
        let (voice_tx, mut voice_rx) = mpsc::channel(20);
        let mut ping_timer = interval(Duration::from_secs(2)).fuse();
        let mut close_callback = None;

        let encoder_handle = async_std::task::spawn(encoder(voice_tx, self.output.clone()));

        loop {
            select! {
                _timestamp = ping_timer.next() => {
                    if !self.send_ping().await {
                        dbg!();
                        break;
                    }
                }
                msg = self.pipe.next() => {
                    let msg = match msg {
                        None => {
                            dbg!();
                            break;
                        }
                        Some(v) => v,
                    };

                    match msg {
                        MumbleClientMessage::BroadcastMessage { channels, users, text, callback } => {
                            let mut m = msgs::TextMessage::new();
                            m.mut_channel_id()
                                .extend(channels.into_iter().map(|el| el.id()));
                            m.mut_session()
                                .extend(users.into_iter().map(|el| el.session_id()));
                            m.set_message(text.to_string());
                            try_or_break!(self.tcp.send(m.into()).await);
                            let _ = callback.send(());
                        }
                        MumbleClientMessage::SetComment { comment, callback } => {
                            let mut state = msgs::UserState::new();
                            state.set_comment(comment);
                            try_or_break!(self.tcp.send(state.into()).await);
                            let _ = callback.send(());
                        }
                        MumbleClientMessage::MyUser { callback } => {
                            let _ = callback.send(self.me.get(&self.server_state).expect("failed to find my user"));
                        }
                        MumbleClientMessage::MyUserRef { callback } => {
                            let _ = callback.send(self.me);
                        }
                        MumbleClientMessage::MyChannel { callback } => {
                            let user = self.me.get(&self.server_state).expect("failed to find my user");
                            let _ = callback.send(user.channel().get(&self.server_state).expect("my user is not in a channel?"));
                        }
                        MumbleClientMessage::MyChannelRef { callback } => {
                            let user = self.me.get(&self.server_state).expect("failed to find my user");
                            let _ = callback.send(user.channel());
                        }
                        MumbleClientMessage::GetUser { r, callback } => {
                            let user = r.get(&self.server_state);
                            let _ = callback.send(user);
                        }
                        MumbleClientMessage::State { callback } => {
                            let _ = callback.send(self.server_state.clone());
                        }
                        MumbleClientMessage::MaxMessageLength { callback } => {
                            let _ = callback.send(self.server_state.max_message_length());
                        }
                        MumbleClientMessage::AllowHtmlMessages { callback } => {
                            debug!("unimplemented");
                            let _ = callback.send(None);
                        }
                        MumbleClientMessage::AudioInput { callback } => {
                            let _ = callback.send(self.output_id);
                        }
                        MumbleClientMessage::EventSubscriber { callback } => {
                            let _ = callback.send(self.event_sub.activate_cloned());
                        }
                        MumbleClientMessage::Close { callback } => {
                            close_callback = Some(callback);
                            break;
                        }
                    }
                }
                voice_packet = voice_rx.next() => {
                    let voice_packet = match voice_packet {
                        None => {
                            dbg!();
                            break;
                        }
                        Some(v) => v,
                    };

                    let packet = VoicePacket::Audio {
                        _dst: Default::default(),
                        target: 0,
                        session_id: (),
                        seq_num: self.audio_seq,
                        payload: voice_packet,
                        position_info: None,
                    };

                    try_or_break!(self.udp.send((packet, self.peer)).await);

                    self.audio_seq += 1;
                }
                msg = self.tcp.next() => {
                    let msg = match msg {
                        None => {
                            dbg!();
                            break;
                        }
                        Some(v) => v,
                    };

                    match msg {
                        Ok(msg) => match msg {
                            ControlPacket::UDPTunnel(p) => self.handle_voice_packet(*p).await,
                            x @ _ => self.handle_control_packet(x).await,
                        },
                        Err(e) => {
                            error!("error receiving TCP packet: {}", e);
                        }
                    }
                }
                msg = self.udp.next() => {
                    let msg = match msg {
                        None => {
                            dbg!();
                            break;
                        }
                        Some(v) => v,
                    };

                    match msg {
                        Ok((msg, _)) => {
                            self.handle_voice_packet(msg).await;
                        }
                        Err(e) => {
                            error!("error receiving UDP packet: {}", e);
                        }
                    }
                }
            }
        }

        let _ = self.tcp.close().await;
        let _ = self.udp.close().await;
        encoder_handle.cancel().await;

        if let Some(close_callback) = close_callback {
            let _ = close_callback.send(());
        }
    }

    async fn send_ping(&mut self) -> bool {
        let utime = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut msg = msgs::Ping::new();
        msg.set_timestamp(utime);
        if let Err(e) = self.tcp.send(msg.into()).await {
            error!("failed to send ping: {}", e);
            return false;
        }

        let msg = VoicePacket::Ping { timestamp: utime };
        if let Err(e) = self.udp.send((msg, self.peer)).await {
            error!("failed to send UDP ping: {}", e);
            return false;
        }

        true
    }

    async fn handle_control_packet(&mut self, msg: ControlPacket<Clientbound>) {
        match msg {
            ControlPacket::Ping(p) => self.handle_ping(*p).await,
            ControlPacket::UserState(p) => self.handle_user_state(*p),
            ControlPacket::UserRemove(p) => self.handle_user_remove(*p),
            ControlPacket::ChannelState(p) => self.handle_channel_state(*p),
            ControlPacket::ChannelRemove(p) => self.handle_channel_remove(*p),
            ControlPacket::TextMessage(p) => self.handle_text_message(*p),
            ControlPacket::ServerConfig(p) => self.handle_server_config(*p),
            _ => {
                debug!("Unhandled packet: {:?}", msg);
            }
        }
    }

    async fn handle_voice_packet(&mut self, msg: VoicePacket<Clientbound>) {
        match msg {
            VoicePacket::Ping { .. } => {}
            VoicePacket::Audio { .. } => {}
        }
    }

    async fn handle_ping(&mut self, _msg: msgs::Ping) {
        // TODO
    }

    fn handle_user_state(&mut self, msg: msgs::UserState) {
        self.server_state.update_user(msg);
    }

    fn handle_user_remove(&mut self, msg: msgs::UserRemove) {
        self.server_state.remove_user(msg.get_session());
    }

    fn handle_channel_state(&mut self, msg: msgs::ChannelState) {
        self.server_state.update_channel(msg);
    }

    fn handle_channel_remove(&mut self, msg: msgs::ChannelRemove) {
        self.server_state.remove_channel(msg.get_channel_id());
    }

    fn handle_text_message(&mut self, mut msg: msgs::TextMessage) {
        let actor = if msg.has_actor() {
            Some(UserRef::new(msg.get_actor()))
        } else {
            None
        };
        let receivers = msg.get_session().iter().map(|v| UserRef::new(*v)).collect();
        let channels = msg
            .get_channel_id()
            .iter()
            .map(|v| ChannelRef::new(*v))
            .collect();
        let message = msg.take_message();

        let event = Event::Message(Message {
            actor,
            receivers,
            channels,
            message,
        });

        let _ = self.event_chan.broadcast(event);
    }

    fn handle_server_config(&mut self, msg: msgs::ServerConfig) {
        self.server_state.update_server_config(msg);
    }
}
