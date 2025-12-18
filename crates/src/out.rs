use std::{path::Path, time::Duration};

use cpal::{
    BuildStreamError, Device, FromSample, SizedSample, StreamConfig,
    traits::{DeviceTrait, StreamTrait},
};
use hound::{WavSpec, WavWriter};

use crate::{LegatoApp, runtime::Runtime};

use assert_no_alloc::*;

pub fn render(
    mut app: LegatoApp,
    path: &Path,
    time: Duration,
) -> Result<(), hound::Error> {
    let config = app.get_config();

    let dur_in_samples = (time.as_secs_f32() * config.sample_rate as f32) as usize;

    let channels = config.channels;

    let spec = WavSpec {
        channels: channels as u16,
        sample_rate: config.sample_rate as u32,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };

    let mut writer = WavWriter::create(path, spec).unwrap();

    let block_size = config.audio_block_size;

    let mut count = 0_usize;

    while count < dur_in_samples {
        let block = app.next_block(None);

        for n in 0..block_size {
            for block_chan in block {
                writer.write_sample(block_chan[n]).unwrap();
            }
        }
        count += block_size;
    }

    writer.finalize().unwrap();

    Ok(())
}

#[inline(always)]
fn write_runtime_data_cpal<T>(output: &mut [T], config: &StreamConfig, runtime: &mut Runtime)
where
    T: SizedSample + FromSample<f64>,
{
    let next_block = runtime.next_block(None);

    let chans = config.channels as usize;

    for (frame_index, frame) in output.chunks_mut(chans).enumerate() {
        for (channel, sample) in frame.iter_mut().enumerate() {
            let pipeline_next_frame = &next_block[channel];
            *sample = T::from_sample(pipeline_next_frame[frame_index] as f64);
        }
    }
}

pub fn start_runtime_audio_thread(
    device: &Device,
    config: StreamConfig,
    mut runtime: Runtime,
) -> Result<(), BuildStreamError> {
    let stream = device.build_output_stream(
        &config.clone(),
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            assert_no_alloc(|| write_runtime_data_cpal(data, &config, &mut runtime))
        },
        |err| eprintln!("An output stream error occurred: {}", err),
        None,
    )?;

    stream.play().unwrap();

    std::thread::park();

    Ok(())
}

#[inline(always)]
fn write_runtime_data_cpal_app<T>(output: &mut [T], config: &StreamConfig, app: &mut LegatoApp)
where
    T: SizedSample + FromSample<f64>,
{
    let next_block = app.next_block(None);

    let chans = config.channels as usize;

    for (frame_index, frame) in output.chunks_mut(chans).enumerate() {
        for (channel, sample) in frame.iter_mut().enumerate() {
            let pipeline_next_frame = &next_block[channel];
            *sample = T::from_sample(pipeline_next_frame[frame_index] as f64);
        }
    }
}

pub fn start_application_audio_thread(
    device: &Device,
    config: StreamConfig,
    mut app: LegatoApp,
) -> Result<(), BuildStreamError> {
    let stream = device.build_output_stream(
        &config.clone(),
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            assert_no_alloc(|| write_runtime_data_cpal_app(data, &config, &mut app))
        },
        |err| eprintln!("An output stream error occurred: {}", err),
        None,
    )?;

    stream.play().unwrap();

    std::thread::park();

    Ok(())
}
