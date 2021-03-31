use std::fmt::Debug;
use std::ops::Add;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::SystemTime;

use futures::{Sink, SinkExt, StreamExt};
use log::debug;
use mumble_protocol::control::msgs;
use mumble_protocol::control::ControlPacket;
use mumble_protocol::voice::VoicePacket;
use mumble_protocol::Clientbound;
use tokio::sync::Mutex;
use tokio::time;
use tokio::time::{timeout_at, Duration, Instant};

use crate::mumble::{AudioSink, AudioSource, PacketSink, PacketSource, AudioState};

pub async fn tcp_handler(running: Arc<AtomicBool>, mut stream: PacketSource) {
    while running.load(Ordering::Relaxed) {
        if let Ok(s) = timeout_at(Instant::now().add(Duration::from_secs(2)), stream.next()).await {
            match s {
                None => {
                    // stream closed
                    running.store(false, Ordering::Relaxed);
                }
                Some(packet) => {
                    let packet = packet.unwrap();

                    handle_packet(packet).await;
                }
            }
        }
    }
}

pub async fn handle_packet(packet: ControlPacket<Clientbound>) {
    match packet {
        ControlPacket::Ping(msg) => {
            debug!("Pong! {:?}", msg);
        }
        x => {
            debug!("Unhandled packet: {:?}", x);
        }
    }
}

pub async fn udp_handler(running: Arc<AtomicBool>, mut stream: AudioSource) {
    while running.load(Ordering::Relaxed) {
        if let Ok(s) = timeout_at(Instant::now().add(Duration::from_secs(2)), stream.next()).await {
            match s {
                None => {
                    // stream closed
                    running.store(false, Ordering::Relaxed);
                }
                Some(packet) => {
                    let (packet, _) = packet.unwrap();

                    handle_voice(packet).await;
                }
            }
        }
    }
}

pub async fn handle_voice(packet: VoicePacket<Clientbound>) {
    match packet {
        VoicePacket::Ping { timestamp } => {
            debug!("UDP Pong! {}", timestamp);
        }
        VoicePacket::Audio { .. } => {
            // debug!("received audio frame!");
        }
    }
}

pub async fn tcp_keepalive(running: Arc<AtomicBool>, sink: Arc<Mutex<PacketSink>>) {
    any_keepalive(running, sink, |utime| {
        let mut msg = msgs::Ping::new();
        msg.set_timestamp(utime);
        msg.into()
    })
    .await;
}

pub async fn udp_keepalive(running: Arc<AtomicBool>, sink: Arc<Mutex<AudioState>>) {
    any_keepalive(running, sink, |utime| VoicePacket::Ping {
        timestamp: utime,
    })
    .await;
}

async fn any_keepalive<F, O, S>(running: Arc<AtomicBool>, sink: Arc<Mutex<S>>, packet_creator: F)
where
    S: Sink<O> + Unpin,
    S::Error: Debug,
    O: Debug,
    F: Fn(u64) -> O,
{
    let mut interval = time::interval(Duration::from_secs(5));

    while running.load(Ordering::Relaxed) {
        interval.tick().await;
        let utime = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let msg = packet_creator(utime);
        let mut sink = sink.lock().await;
        debug!("Ping! {:?}", msg);
        sink.send(msg.into()).await.unwrap();
    }
}
