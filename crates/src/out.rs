use std::{path::Path, time::Duration};

use cpal::{
    BuildStreamError, FromSample, SizedSample, StreamConfig,
    traits::{DeviceTrait, StreamTrait},
};
use hound::{WavSpec, WavWriter};

use crate::LegatoApp;
#[cfg(feature = "cpal-backend")]
use crate::interface::AudioInterface;

use assert_no_alloc::*;

pub fn render(mut app: LegatoApp, path: &Path, time: Duration) -> Result<(), hound::Error> {
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

    let block_size = config.block_size;

    let mut count = 0_usize;

    while count < dur_in_samples {
        let block_view = app.next_block(None);
        let block = &block_view.channels[0..block_view.chans];

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

#[cfg(feature = "cpal-backend")]
#[inline(always)]
fn write_runtime_data_cpal_app<T>(output: &mut [T], config: &StreamConfig, app: &mut LegatoApp)
where
    T: SizedSample + FromSample<f64>,
{
    let next_block_view = app.next_block(None);
    let next_block = &next_block_view.channels[0..next_block_view.chans];

    let chans = config.channels as usize;

    for (frame_index, frame) in output.chunks_mut(chans).enumerate() {
        for (channel, sample) in frame.iter_mut().enumerate() {
            let pipeline_next_frame = &next_block[channel];
            *sample = T::from_sample(pipeline_next_frame[frame_index] as f64);
        }
    }
}

#[cfg(feature = "cpal-backend")]
pub fn start_application_audio_thread(
    interface: AudioInterface,
    mut app: LegatoApp,
) -> Result<(), BuildStreamError> {
    let stream = interface.device.build_output_stream(
        &interface.stream_config.clone(),
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            assert_no_alloc(|| {
                write_runtime_data_cpal_app(data, &interface.stream_config, &mut app)
            })
        },
        |err| eprintln!("An output stream error occurred: {}", err),
        None,
    )?;

    stream.play().unwrap();

    std::thread::park();

    Ok(())
}

// ----------------------------------------------
// Version with external output for visualization
// ----------------------------------------------

#[cfg(feature = "cpal-backend")]
#[inline(always)]
fn write_runtime_data_cpal_external_out<T>(
    output: &mut [T],
    config: &StreamConfig,
    app: &mut LegatoApp,
    producer: &mut rtrb::Producer<f32>,
) where
    T: SizedSample + FromSample<f64>,
{
    let block_size = app.get_config().block_size;

    let next_block_view = app.next_block(None);
    let next_block = &next_block_view.channels[0..next_block_view.chans];

    let chans = config.channels as usize;

    // Write out to visualization thread.
    // TODO: Chunk utilities can speed this up
    for i in 0..block_size {
        let sample = (next_block[0][i] + next_block[1][i]) * 0.5;
        let _ = producer.push(sample);
    }

    for (frame_index, frame) in output.chunks_mut(chans).enumerate() {
        for (channel, sample) in frame.iter_mut().enumerate() {
            let pipeline_next_frame = &next_block[channel];
            *sample = T::from_sample(pipeline_next_frame[frame_index] as f64);
        }
    }
}

#[cfg(feature = "cpal-backend")]
pub fn start_application_audio_thread_external_output(
    interface: AudioInterface,
    mut output_producer: rtrb::Producer<f32>,
    mut app: LegatoApp,
) -> Result<(), BuildStreamError> {
    let cfg = interface.stream_config.clone();
    let stream = interface.device.build_output_stream(
        &interface.stream_config,
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            assert_no_alloc(|| {
                write_runtime_data_cpal_external_out(data, &cfg, &mut app, &mut output_producer)
            })
        },
        |err| eprintln!("An output stream error occurred: {}", err),
        None,
    )?;

    stream.play().unwrap();

    std::thread::park();

    Ok(())
}
