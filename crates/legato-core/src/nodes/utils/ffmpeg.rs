use std::{
    io::{BufReader, Read},
    process::{Command, Stdio},
};

use crate::engine::resources::audio_sample::AudioSample;

// For the time being, we're just using FFMPEG for loading samples.
// We can do something better in the future if required, i.e streaming with channel.
pub fn decode_with_ffmpeg(path: &str, chans: usize, sr: u32) -> std::io::Result<AudioSample> {
    let mut child = Command::new("ffmpeg")
        .args([
            "-i",
            path, // input
            "-f",
            "f32le", // correct format for f32
            "-ac",
            &chans.to_string(), // number of channels
            "-ar",              // sample rate
            &sr.to_string(),
            "-acodec",
            "pcm_f32le",
            "pipe:1",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null()) // silence ffmpeg logging
        .spawn()?;

    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    // Prepare per-channel storage
    let mut per_channel = vec![Vec::new(); chans];

    let mut buf = [0u8; 4]; // one f32 sample
    let mut channel_idx = 0;

    while reader.read_exact(&mut buf).is_ok() {
        let sample = f32::from_le_bytes(buf);
        per_channel[channel_idx].push(sample);

        channel_idx += 1;
        if channel_idx == chans {
            channel_idx = 0;
        }
    }

    Ok(AudioSample::new(chans, per_channel))
}
