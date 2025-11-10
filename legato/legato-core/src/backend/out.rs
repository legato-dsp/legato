use std::{ops::Mul, path::Path, time::Duration};

use cpal::{
    traits::{DeviceTrait, StreamTrait},
    BuildStreamError, Device, FromSample, SizedSample, StreamConfig,
};
use generic_array::ArrayLength;
use hound::{WavSpec, WavWriter};
use typenum::{Prod, U0, U2};

use crate::engine::{node::FrameSize, runtime::Runtime};

pub fn render<AF, CF, C>(
    mut runtime: Runtime<AF, CF, C, U0>,
    path: &Path,
    sr: u32,
    time: Duration,
) -> Result<(), hound::Error>
where
    AF: FrameSize + Mul<U2>,
    Prod<AF, U2>: FrameSize,
    CF: FrameSize,
    C: ArrayLength,
{
    let dur_in_samples = (time.as_secs_f32() * sr as f32) as usize;
    let mut count = 0_usize;

    let spec = WavSpec {
        channels: C::to_u16(),
        sample_rate: sr,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };

    let mut writer = WavWriter::create(path, spec).unwrap();

    while count < dur_in_samples {
        let block = runtime.next_block(None);

        for n in 0..AF::USIZE {
            for c in 0..C::USIZE {
                writer.write_sample(block[c][n]).unwrap();
            }
        }
        count += AF::USIZE;
    }

    writer.finalize().unwrap();

    Ok(())
}

#[inline(always)]
fn write_data_cpal<AF, CF, C, Ci, T>(output: &mut [T], runtime: &mut Runtime<AF, CF, C, Ci>)
where
    T: SizedSample + FromSample<f64>,
    AF: FrameSize + Mul<U2>,
    Prod<AF, U2>: FrameSize,
    CF: FrameSize,
    Ci: ArrayLength,
    C: ArrayLength,
{
    let next_block = runtime.next_block(None);

    for (frame_index, frame) in output.chunks_mut(C::USIZE).enumerate() {
        for (channel, sample) in frame.iter_mut().enumerate() {
            let pipeline_next_frame = &next_block[channel];
            *sample = T::from_sample(pipeline_next_frame[frame_index] as f64);
        }
    }
}

pub fn start_audio_thread<AF, CF, C, Ci>(
    device: &Device,
    config: &StreamConfig,
    mut runtime: Runtime<AF, CF, C, Ci>,
) -> Result<(), BuildStreamError>
where
    AF: FrameSize + Mul<U2>,
    Prod<AF, U2>: FrameSize,
    CF: FrameSize,
    Ci: ArrayLength,
    C: ArrayLength,
{
    let stream = device.build_output_stream(
        config,
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            // assert_no_alloc(|| write_data_cpal::<AF, CF, C, f32>(data, &mut runtime))
            write_data_cpal(data, &mut runtime);
        },
        |err| eprintln!("An output stream error occurred: {}", err),
        None,
    )?;

    stream.play().unwrap();

    std::thread::park();

    Ok(())
}
