use std::fmt::Debug;
use std::io;
use std::net::SocketAddr;
use std::time::{Duration, SystemTime};

use futures::{Sink, SinkExt, Stream, StreamExt};
use log::{debug, error};
use mumble_protocol::control::{msgs, ControlPacket};
use mumble_protocol::voice::{VoicePacket, VoicePacketPayload};
use mumble_protocol::{Clientbound, Serverbound};
use tokio::select;
use tokio::sync::{mpsc, watch};
use tokio::time::interval;

pub use encoder::encoder;

use crate::mumble::event::Event;
use crate::mumble::server_state::{ChannelRef, UserRef};
use crate::mumble::state::ClientData;

mod encoder;

pub async fn main_task<T, U>(
    data: ClientData,
    mut tcp_rx: T,
    mut udp_rx: U,
    mut voice_rx: mpsc::Receiver<VoicePacketPayload>,
    mut stop_recv: watch::Receiver<()>,
) where
    T: Stream<Item = io::Result<ControlPacket<Clientbound>>> + Unpin,
    U: Stream<Item = io::Result<(VoicePacket<Clientbound>, SocketAddr)>> + Unpin,
{
    let mut ping_timer = interval(Duration::from_secs(2));
    let mut seq = 0;

    loop {
        select! {
            msg = tcp_rx.next() => handle_tcp(&data, msg).await,
            msg = udp_rx.next() => handle_udp(&data, msg).await,
            frame = voice_rx.recv() => handle_voice_frame(&data, &mut seq, frame).await,
            _ = ping_timer.tick() => ping(&data).await,
            _ = stop_recv.changed() => break,
        }
    }
}

pub async fn tcp_sender<T>(mut source: mpsc::Receiver<ControlPacket<Serverbound>>, mut socket: T)
where
    T: Sink<ControlPacket<Serverbound>> + Unpin,
    T::Error: Debug,
{
    while let Some(packet) = source.recv().await {
        socket.send(packet).await.unwrap();
    }
}

pub async fn udp_sender<T>(mut source: mpsc::Receiver<VoicePacket<Serverbound>>, mut socket: T, addr: SocketAddr)
where
    T: Sink<(VoicePacket<Serverbound>, SocketAddr)> + Unpin,
    T::Error: Debug,
{
    while let Some(packet) = source.recv().await {
        socket.send((packet, addr)).await.unwrap();
    }
}

async fn ping(data: &ClientData) {
    let utime = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let mut msg = msgs::Ping::new();
    msg.set_timestamp(utime);
    data.tcp_tx.send(msg.into()).await.unwrap();

    let msg = VoicePacket::Ping { timestamp: utime };
    data.udp_tx.send(msg).await.unwrap();
}

async fn handle_tcp(data: &ClientData, msg: Option<io::Result<ControlPacket<Clientbound>>>) {
    match msg {
        None => {
            todo!("handle disconnection")
        }
        Some(Ok(p)) => match p {
            ControlPacket::UDPTunnel(p) => handle_voice_packet(data, *p).await,
            x @ _ => handle_control_packet(data, x).await,
        },
        Some(Err(e)) => {
            error!("failed to receive TCP packet: {}", e)
        }
    }
}

async fn handle_udp(
    data: &ClientData,
    msg: Option<io::Result<(VoicePacket<Clientbound>, SocketAddr)>>,
) {
    match msg {
        None => {
            todo!("handle disconnection")
        }
        Some(Ok((p, _))) => handle_voice_packet(data, p).await,
        Some(Err(e)) => {
            error!("failed to receive UDP packet: {}", e)
        }
    }
}

async fn handle_voice_frame(data: &ClientData, seq: &mut u64, frame: Option<VoicePacketPayload>) {
    let frame = frame.unwrap();

    let packet = VoicePacket::Audio {
        _dst: Default::default(),
        target: 0,
        session_id: (),
        seq_num: *seq,
        payload: frame,
        position_info: None,
    };

    data.udp_tx.send(packet).await.unwrap();

    *seq += 1;
}

async fn handle_control_packet(data: &ClientData, msg: ControlPacket<Clientbound>) {
    match msg {
        ControlPacket::Ping(p) => handle_ping(data, *p).await,
        ControlPacket::UserState(p) => handle_user_state(data, *p).await,
        ControlPacket::UserRemove(p) => handle_user_remove(data, *p).await,
        ControlPacket::ChannelState(p) => handle_channel_state(data, *p).await,
        ControlPacket::ChannelRemove(p) => handle_channel_remove(data, *p).await,
        ControlPacket::TextMessage(p) => handle_text_message(data, *p).await,
        _ => {
            debug!("Unhandled packet: {:?}", msg);
        }
    }
}

async fn handle_voice_packet(data: &ClientData, msg: VoicePacket<Clientbound>) {
    match msg {
        VoicePacket::Ping { .. } => {}
        VoicePacket::Audio { .. } => {}
    }
}

async fn handle_ping(data: &ClientData, msg: msgs::Ping) {
    // TODO
}

async fn handle_user_state(data: &ClientData, msg: msgs::UserState) {
    data.server_state.lock().await.update_user(msg);
}

async fn handle_user_remove(data: &ClientData, msg: msgs::UserRemove) {
    data.server_state
        .lock()
        .await
        .remove_user(msg.get_session());
}

async fn handle_channel_state(data: &ClientData, msg: msgs::ChannelState) {
    data.server_state.lock().await.update_channel(msg);
}

async fn handle_channel_remove(data: &ClientData, msg: msgs::ChannelRemove) {
    data.server_state
        .lock()
        .await
        .remove_channel(msg.get_channel_id());
}

async fn handle_text_message(data: &ClientData, mut msg: msgs::TextMessage) {
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

    let event = Event::Message {
        actor,
        receivers,
        channels,
        message,
    };

    let _ = data.event_chan.send(event);
}
