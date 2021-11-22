use std::sync::Arc;
use std::time::Duration;

use audiopus::{Application, Channels, SampleRate};
use bytes::Bytes;
use dasp::sample::ToSample;
use dasp::{Frame, Sample, Signal};
use log::debug;
use mumble_protocol::voice::VoicePacketPayload;
use tokio::select;
use tokio::sync::{mpsc, watch, Mutex};
use tokio::time;

pub(super) async fn encoder<S>(
    voice_tx: mpsc::Sender<VoicePacketPayload>,
    pipe: Arc<Mutex<S>>,
    // mut stop_recv: watch::Receiver<()>,
) where
    S: Signal,
    <S::Frame as Frame>::Sample: ToSample<i16>,
{
    let mut pipe = pipe.lock().await;

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

    let op = async move {
        let mut last_was_empty = true;

        loop {
            interval.tick().await;

            let mut is_empty = true;

            for (idx, frame) in pipe.by_ref().take(pcm_buf.len()).enumerate() {
                // adjust volume
                // let frame = frame.map(|s| s.to_sample() as i16).scale_amp(0.1);

                // TODO: handle more than left channel
                let ch0 = frame.channel(0).unwrap();
                let sample = ch0.to_sample().scale_amp(0.1);

                if sample != 0 {
                    is_empty = false;
                }

                pcm_buf[idx] = sample;
            }

            if !(is_empty && last_was_empty) {
                let len = encoder.encode(&pcm_buf, &mut opus_buf).unwrap();

                let _ = voice_tx
                    .send(VoicePacketPayload::Opus(
                        Bytes::copy_from_slice(&opus_buf[..len]),
                        is_empty,
                    ))
                    .await;
            }

            last_was_empty = is_empty;
        }
    };

    select! {
        _ = op => {}
        // _ = stop_recv.changed() => {}
    }

    debug!("encoder exit");
}
