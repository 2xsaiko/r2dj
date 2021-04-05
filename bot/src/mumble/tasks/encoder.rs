use std::time::Duration;

use audiopus::{Application, Channels, SampleRate};
use bytes::Bytes;
use mumble_protocol::voice::VoicePacketPayload;
use tokio::io::AsyncReadExt;
use tokio::time;

use crate::mixer::MixerOutput;
use crate::mumble::state::ClientData;
use crate::util::slice_to_u8_mut;

pub async fn encoder(data: ClientData, mut pipe: MixerOutput) {
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

    let mut extra_byte = false;

    loop {
        interval.tick().await;

        let u8_buf = if extra_byte {
            &mut slice_to_u8_mut(&mut pcm_buf)[1..]
        } else {
            slice_to_u8_mut(&mut pcm_buf)
        };

        let r = pipe.read(u8_buf).await;

        if let Ok(mut r) = r {
            if r == 0 {
                // self.send_audio_frame(VoicePacketPayload::Opus(Bytes::new(), true))
                //     .await?;

                break;
            }

            if extra_byte {
                r += 1;
            }

            // divide by 2 since this is the size in bytes
            let input_len = r / 2;

            // adjust volume
            for el in pcm_buf[..input_len].iter_mut() {
                *el = (*el as f32 * 0.1) as i16;
            }

            // OPUS does not like encoding less data, so let's fill the rest
            // with zeroes and send over the whole buffer >_>

            if input_len < pcm_buf.len() {
                pcm_buf[input_len..].fill(0);
            }

            let len = encoder.encode(&pcm_buf, &mut opus_buf).unwrap();

            data.voice_tx
                .send(VoicePacketPayload::Opus(
                    Bytes::copy_from_slice(&opus_buf[..len]),
                    input_len < pcm_buf.len(),
                ))
                .await
                .unwrap();

            if r % 2 != 0 {
                let u8_buf = slice_to_u8_mut(&mut pcm_buf);
                u8_buf[0] = u8_buf[r - 1];
                extra_byte = true;
            } else {
                extra_byte = false;
            }
        }
    }
}
