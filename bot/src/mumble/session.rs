use std::fmt::Debug;
use std::sync::Arc;
use std::time::SystemTime;

use futures::{Sink, SinkExt, StreamExt};
use log::debug;
use mumble_protocol::control::msgs;
use mumble_protocol::control::ControlPacket;
use mumble_protocol::voice::VoicePacket;
use mumble_protocol::Clientbound;
use tokio::select;
use tokio::sync::Mutex;
use tokio::sync::{broadcast, watch};
use tokio::time::{interval, Duration};

use crate::mumble::server_state::{ChannelRef, ServerState, UserRef};
use crate::mumble::{AudioSource, AudioState, Event, PacketSink, PacketSource};

pub(super) async fn tcp_handler(
    mut shutdown_channel: watch::Receiver<()>,
    mut stream: PacketSource,
    sink: Arc<Mutex<PacketSink>>,
    server_state: Arc<Mutex<ServerState>>,
    mut event_chan: broadcast::Sender<Event>,
) {
    let mut ping_timer = interval(Duration::from_secs(5));
    loop {
        select! {
            s = stream.next() => {
                match s {
                    None => {
                        // stream closed
                        // running.store(false, Ordering::Relaxed);
                        todo!("shutdown channel")
                    }
                    Some(packet) => {
                        let packet = packet.unwrap();

                        handle_packet(&server_state, &mut event_chan, packet).await;
                    }
                }
            }
            _ = ping_timer.tick() => send_keepalive(&mut *sink.lock().await, |utime| {
                let mut msg = msgs::Ping::new();
                msg.set_timestamp(utime);
                msg.into()
            }).await,
            _ = shutdown_channel.changed() => break,
        }
    }
}

pub async fn handle_packet(
    server_state: &Arc<Mutex<ServerState>>,
    event_chan: &mut broadcast::Sender<Event>,
    packet: ControlPacket<Clientbound>,
) {
    match packet {
        ControlPacket::Ping(_) => {}
        ControlPacket::UserState(p) => {
            let mut st = server_state.lock().await;
            st.update_user(*p);
        }
        ControlPacket::UserRemove(p) => {
            let mut st = server_state.lock().await;
            st.remove_user(p.get_session());
        }
        ControlPacket::ChannelState(p) => {
            let mut st = server_state.lock().await;
            st.update_channel(*p);
        }
        ControlPacket::ChannelRemove(p) => {
            let mut st = server_state.lock().await;
            st.remove_channel(p.get_channel_id());
        }
        ControlPacket::TextMessage(mut p) => {
            let actor = if p.has_actor() {
                Some(UserRef::new(p.get_actor()))
            } else {
                None
            };
            let receivers = p
                .get_session()
                .iter()
                .map(|v| UserRef::new(*v))
                .collect();
            let channels = p
                .get_channel_id()
                .iter()
                .map(|v| ChannelRef::new(*v))
                .collect();
            let message = p.take_message();

            let event = Event::Message {
                actor,
                receivers,
                channels,
                message,
            };

            let _ = event_chan.send(event);
        }
        x => {
            debug!("Unhandled packet: {:?}", x);
        }
    }
}

pub async fn udp_handler(
    mut shutdown_channel: watch::Receiver<()>,
    mut stream: AudioSource,
    sink: Arc<Mutex<AudioState>>,
) {
    let mut interval = interval(Duration::from_secs(5));
    loop {
        select! {
            s = stream.next() => {
                match s {
                    None => {
                        // stream closed
                        // running.store(false, Ordering::Relaxed);
                        todo!("shutdown channel")
                    }
                    Some(packet) => {
                        let (packet, _) = packet.unwrap();

                        handle_voice(packet).await;
                    }
                }
            }
            _ = interval.tick() => send_keepalive(&mut *sink.lock().await, |utime| VoicePacket::Ping {
                timestamp: utime,
            }).await,
            _ = shutdown_channel.changed() => break,
        }
    }
}

pub async fn handle_voice(packet: VoicePacket<Clientbound>) {
    match packet {
        VoicePacket::Ping { .. } => {}
        VoicePacket::Audio { .. } => {}
    }
}

async fn send_keepalive<F, O, S>(sink: &mut S, packet_creator: F)
where
    S: Sink<O> + Unpin,
    S::Error: Debug,
    O: Debug,
    F: Fn(u64) -> O,
{
    let utime = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let msg = packet_creator(utime);
    sink.send(msg.into()).await.unwrap();
}
