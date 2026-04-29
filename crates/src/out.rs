use std::{path::Path, time::Duration};

use hound::{WavSpec, WavWriter};

use crate::LegatoApp;

/// Just render out to a .wav file, more used for testing for the time being.
/// 
/// TODO: In the future, we will have a dedicated writer thread
pub fn render(mut app: LegatoApp, path: &Path, time: Duration) -> Result<(), hound::Error> {
    let config = app.get_config();
    let dur_in_samples = (time.as_secs_f32() * config.sample_rate as f32) as usize;

    let spec = WavSpec {
        channels: config.channels as u16,
        sample_rate: config.sample_rate as u32,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };

    let mut writer = WavWriter::create(path, spec)?;
    let block_size = config.block_size;
    let mut count = 0_usize;

    while count < dur_in_samples {
        let block_view = app.next_block(None);
        let block = &block_view.channels[0..block_view.chans];

        for n in 0..block_size {
            for block_chan in block {
                writer.write_sample(block_chan[n])?;
            }
        }
        count += block_size;
    }

    writer.finalize()?;
    Ok(())
}