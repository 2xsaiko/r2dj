use std::sync::Arc;

use mumble_protocol::control::ControlPacket;
use mumble_protocol::Serverbound;
use tokio::sync::{broadcast, Mutex, mpsc};

use crate::mixer::MixerInput;
use crate::mumble::server_state::{ServerState, UserRef};
use mumble_protocol::voice::{VoicePacket, VoicePacketPayload};
use crate::mumble::event::Event;

#[derive(Debug, Clone)]
pub struct ClientData {
    pub server_state: Arc<Mutex<ServerState>>,
    pub session: UserRef,
    pub event_chan: broadcast::Sender<Event>,
    pub mixer: MixerInput,
    pub voice_tx: mpsc::Sender<VoicePacketPayload>,

    // networking stuff
    pub tcp_tx: mpsc::Sender<ControlPacket<Serverbound>>,
    pub udp_tx: mpsc::Sender<VoicePacket<Serverbound>>,
}
