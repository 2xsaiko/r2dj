use std::fmt::Debug;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use futures::{Sink, SinkExt, Stream, StreamExt, TryFutureExt};
use log::{debug, error};
use mumble_protocol::control::{msgs, ControlPacket};
use mumble_protocol::voice::{VoicePacket, VoicePacketPayload};
use mumble_protocol::{Clientbound, Serverbound};
use petgraph::graph::NodeIndex;
use tokio::select;
use tokio::sync::{broadcast, mpsc, watch, Mutex as AsyncMutex};
use tokio::task::JoinHandle;
use tokio::time::interval;

use audiopipe::aaaaaaa::{Core, OutputSignal};
use audiopipe::mixer::{new_mixer, MixerInput, MixerOutput};
use encoder::encoder;

use crate::event::Event;
use crate::server_state::{ChannelRef, ServerState, UserRef};

mod encoder;

pub struct ConnectionInfo<T, U> {
    tcp: T,
    udp: U,
    addr: SocketAddr,
    server_state: Arc<Mutex<ServerState>>,
}

impl<T, U> ConnectionInfo<T, U> {
    pub fn new(tcp: T, udp: U, addr: SocketAddr, server_state: Arc<Mutex<ServerState>>) -> Self {
        ConnectionInfo {
            tcp,
            udp,
            addr,
            server_state,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Connectors {
    m_in: NodeIndex,
    cp_tx: mpsc::Sender<ControlPacket<Serverbound>>,
    event_chan: broadcast::Sender<Event>,
    stop_recv: watch::Receiver<()>,

    // Private stuff
    cp_rx: Arc<AsyncMutex<mpsc::Receiver<ControlPacket<Serverbound>>>>,
    m_out: Arc<AsyncMutex<OutputSignal<2>>>,
}

impl Connectors {
    pub fn new(stop_recv: watch::Receiver<()>, ac: &Core) -> Self {
        let output = ac.add_output();
        let (cp_tx, cp_rx) = mpsc::channel(20);
        let (event_chan, _) = broadcast::channel(20);

        Connectors {
            m_in: output.node(),
            cp_tx,
            event_chan,
            stop_recv,
            cp_rx: Arc::new(AsyncMutex::new(cp_rx)),
            m_out: Arc::new(AsyncMutex::new(output)),
        }
    }

    pub fn event_subscriber(&self) -> broadcast::Receiver<Event> {
        self.event_chan.subscribe()
    }

    pub fn audio_input(&self) -> NodeIndex {
        self.m_in.clone()
    }

    pub fn cp_tx(&self) -> &mpsc::Sender<ControlPacket<Serverbound>> {
        &self.cp_tx
    }
}

#[derive(Debug, Clone)]
struct InternalConnectors {
    voice_tx: mpsc::Sender<VoicePacketPayload>,
    cp_tx: mpsc::Sender<ControlPacket<Serverbound>>,
    vp_tx: mpsc::Sender<VoicePacket<Serverbound>>,
    event_chan: broadcast::Sender<Event>,
}

#[derive(Debug, Clone)]
struct InternalState {
    server_state: Arc<Mutex<ServerState>>,
    ic: InternalConnectors,
}

pub async fn start_tasks<T, U>(ci: ConnectionInfo<T, U>, stuff: Connectors) -> Vec<JoinHandle<()>>
where
    T: Stream<Item = io::Result<ControlPacket<Clientbound>>>
        + Sink<ControlPacket<Serverbound>>
        + Send
        + Unpin
        + 'static,
    T::Error: Debug,
    U: Stream<Item = io::Result<(VoicePacket<Clientbound>, SocketAddr)>>
        + Sink<(VoicePacket<Serverbound>, SocketAddr)>
        + Send
        + Unpin
        + 'static,
    U::Error: Debug,
{
    // no partial borrows :(
    let Connectors {
        cp_tx,
        event_chan,
        stop_recv,
        cp_rx,
        m_out,
        ..
    } = stuff;
    let ConnectionInfo {
        tcp,
        udp,
        addr,
        server_state,
    } = ci;

    let (tcp_tx, tcp_rx) = tcp.split();
    let (udp_tx, udp_rx) = udp.split();

    let (vp_tx, vp_rx) = mpsc::channel(20);
    let (voice_tx, voice_rx) = mpsc::channel(20);

    let is = InternalState {
        ic: InternalConnectors {
            voice_tx,
            cp_tx,
            vp_tx,
            event_chan,
        },
        server_state: server_state.clone(),
    };

    vec![
        tokio::spawn(pinger(
            is.ic.cp_tx.clone(),
            is.ic.vp_tx.clone(),
            stop_recv.clone(),
        )),
        tokio::spawn(encoder(is.ic.voice_tx.clone(), m_out, stop_recv.clone())),
        tokio::spawn(voice(is.ic.vp_tx.clone(), voice_rx)),
        tokio::spawn(receive_tcp(is.clone(), tcp_rx, stop_recv.clone())),
        tokio::spawn(receive_udp(is.clone(), udp_rx, stop_recv.clone())),
        tokio::spawn(tcp_sender(cp_rx, tcp_tx, stop_recv.clone())),
        tokio::spawn(udp_sender(vp_rx, udp_tx, addr)),
    ]
}

async fn receive_tcp<T>(is: InternalState, mut stream: T, mut stop_recv: watch::Receiver<()>)
where
    T: Stream<Item = io::Result<ControlPacket<Clientbound>>> + Unpin,
{
    let op = async move {
        while let Some(r) = stream.next().await {
            match r {
                Ok(msg) => match msg {
                    ControlPacket::UDPTunnel(p) => handle_voice_packet(&is, *p).await,
                    x @ _ => handle_control_packet(&is, x).await,
                },
                Err(e) => {
                    error!("error receiving TCP packet: {}", e);
                }
            }
        }
    };

    select! {
        _ = op => {}
        _ = stop_recv.changed() => {}
    }

    debug!("receive_tcp exit");
}

async fn receive_udp<T>(is: InternalState, mut stream: T, mut stop_recv: watch::Receiver<()>)
where
    T: Stream<Item = io::Result<(VoicePacket<Clientbound>, SocketAddr)>> + Unpin,
{
    let op = async move {
        while let Some(r) = stream.next().await {
            match r {
                Ok((msg, _)) => {
                    handle_voice_packet(&is, msg).await;
                }
                Err(e) => {
                    error!("error receiving UDP packet: {}", e);
                }
            }
        }
    };

    select! {
        _ = op => {}
        _ = stop_recv.changed() => {}
    }

    debug!("receive_udp exit");
}

async fn voice(
    vp_tx: mpsc::Sender<VoicePacket<Serverbound>>,
    mut voice_rx: mpsc::Receiver<VoicePacketPayload>,
) {
    let mut seq = 0;

    while let Some(frame) = voice_rx.recv().await {
        let packet = VoicePacket::Audio {
            _dst: Default::default(),
            target: 0,
            session_id: (),
            seq_num: seq,
            payload: frame,
            position_info: None,
        };

        vp_tx.send(packet).await.unwrap();

        seq += 1;
    }

    debug!("voice exit");
}

async fn pinger(
    cp_tx: mpsc::Sender<ControlPacket<Serverbound>>,
    vp_tx: mpsc::Sender<VoicePacket<Serverbound>>,
    mut stop_recv: watch::Receiver<()>,
) {
    let mut ping_timer = interval(Duration::from_secs(2));

    let op = async move {
        loop {
            ping_timer.tick().await;

            let utime = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs();

            let mut msg = msgs::Ping::new();
            msg.set_timestamp(utime);
            cp_tx.send(msg.into()).await.unwrap();

            let msg = VoicePacket::Ping { timestamp: utime };
            vp_tx.send(msg).await.unwrap();
        }
    };

    select! {
        _ = stop_recv.changed() => {},
        _ = op => {},
    }

    debug!("pinger exit");
}

async fn tcp_sender<T>(
    source: Arc<AsyncMutex<mpsc::Receiver<ControlPacket<Serverbound>>>>,
    mut socket: T,
    mut stop_recv: watch::Receiver<()>,
) where
    T: Sink<ControlPacket<Serverbound>> + Unpin,
    T::Error: Debug,
{
    let mut source = source.lock().await;

    let op = async {
        while let Some(packet) = source.recv().await {
            socket.send(packet).await.unwrap();
        }
    };

    select! {
        _ = stop_recv.changed() => {},
        _ = op => {},
    }

    socket.flush().await.unwrap();
    socket.close().await.unwrap();

    debug!("tcp sender exit");
}

async fn udp_sender<T>(
    mut source: mpsc::Receiver<VoicePacket<Serverbound>>,
    mut socket: T,
    addr: SocketAddr,
) where
    T: Sink<(VoicePacket<Serverbound>, SocketAddr)> + Unpin,
    T::Error: Debug,
{
    while let Some(packet) = source.recv().await {
        socket.send((packet, addr)).await.unwrap();
    }

    debug!("udp sender exit");
}

async fn handle_control_packet(is: &InternalState, msg: ControlPacket<Clientbound>) {
    match msg {
        ControlPacket::Ping(p) => handle_ping(is, *p).await,
        ControlPacket::UserState(p) => handle_user_state(is, *p),
        ControlPacket::UserRemove(p) => handle_user_remove(is, *p),
        ControlPacket::ChannelState(p) => handle_channel_state(is, *p),
        ControlPacket::ChannelRemove(p) => handle_channel_remove(is, *p),
        ControlPacket::TextMessage(p) => handle_text_message(is, *p),
        _ => {
            debug!("Unhandled packet: {:?}", msg);
        }
    }
}

async fn handle_voice_packet(_is: &InternalState, msg: VoicePacket<Clientbound>) {
    match msg {
        VoicePacket::Ping { .. } => {}
        VoicePacket::Audio { .. } => {}
    }
}

async fn handle_ping(_is: &InternalState, _msg: msgs::Ping) {
    // TODO
}

fn handle_user_state(is: &InternalState, msg: msgs::UserState) {
    is.server_state.lock().unwrap().update_user(msg);
}

fn handle_user_remove(is: &InternalState, msg: msgs::UserRemove) {
    is.server_state.lock().unwrap().remove_user(msg.get_session());
}

fn handle_channel_state(is: &InternalState, msg: msgs::ChannelState) {
    is.server_state.lock().unwrap().update_channel(msg);
}

fn handle_channel_remove(is: &InternalState, msg: msgs::ChannelRemove) {
    is.server_state
        .lock()
        .unwrap()
        .remove_channel(msg.get_channel_id());
}

fn handle_text_message(is: &InternalState, mut msg: msgs::TextMessage) {
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

    let _ = is.ic.event_chan.send(event);
}
