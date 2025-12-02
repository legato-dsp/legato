use std::{ops::Mul, path::Path, time::Duration};

use cpal::{
    BuildStreamError, Device, FromSample, SizedSample, StreamConfig,
    traits::{DeviceTrait, StreamTrait},
};
use hound::{WavSpec, WavWriter};

use crate::runtime::runtime::Runtime;

pub fn render(
    mut runtime: Runtime,
    path: &Path,
    sr: u32,
    time: Duration,
) -> Result<(), hound::Error> {
    let dur_in_samples = (time.as_secs_f32() * sr as f32) as usize;
    let mut count = 0_usize;

    let config = runtime.get_config();
    let channels = config.channels;

    let spec = WavSpec {
        channels: channels as u16,
        sample_rate: sr,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };

    let mut writer = WavWriter::create(path, spec).unwrap();

    let block_size = config.audio_block_size;

    while count < dur_in_samples {
        let block = runtime.next_block(None);

        for n in 0..block_size {
            for c in 0..channels {
                writer.write_sample(block[c][n]).unwrap();
            }
        }
        count += block_size;
    }

    writer.finalize().unwrap();

    Ok(())
}

#[inline(always)]
fn write_runtime_data_cpal<T>(output: &mut [T], runtime: &mut Runtime)
where
    T: SizedSample + FromSample<f64>,
{
    let config = runtime.get_config();

    let next_block = runtime.next_block(None);

    let chans = config.channels;

    for (frame_index, frame) in output.chunks_mut(chans).enumerate() {
        for (channel, sample) in frame.iter_mut().enumerate() {
            let pipeline_next_frame = &next_block[channel];
            *sample = T::from_sample(pipeline_next_frame[frame_index] as f64);
        }
    }
}

pub fn start_runtime_audio_thread(
    device: &Device,
    config: &StreamConfig,
    mut runtime: Runtime,
) -> Result<(), BuildStreamError> {
    let stream = device.build_output_stream(
        config,
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            // assert_no_alloc(|| write_data_cpal(data, &mut runtime))
            write_runtime_data_cpal(data, &mut runtime);
        },
        |err| eprintln!("An output stream error occurred: {}", err),
        None,
    )?;

    stream.play().unwrap();

    std::thread::park();

    Ok(())
}
