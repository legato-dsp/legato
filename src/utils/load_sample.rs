use std::process::{Command, Stdio};
use std::io::{Read, BufReader};
use std::sync::Arc;

pub fn decode_with_ffmpeg<const C: usize>(path: &str) -> std::io::Result<Arc<[Vec<f32>; C]>> {
    // ffmpeg command:
    // -i <file> : input file
    // -f f32le  : output format: 32-bit float, little-endian
    // -ac <ch>  : number of channels
    // -acodec pcm_f32le : codec to use
    // pipe:1    : write to stdout
    let mut child = Command::new("ffmpeg")
        .args([
            "-i", path,
            "-f", "f32le",
            "-ac", &C.to_string(),
            "-acodec", "pcm_f32le",
            "pipe:1"
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null()) // silence ffmpeg logging
        .spawn()?;

    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    // Prepare per-channel storage
    let mut per_channel = [const { Vec::<f32>::new() }; C];

    let mut buf = [0u8; 4]; // one f32 sample
    let mut channel_idx = 0;

    while reader.read_exact(&mut buf).is_ok() {
        let sample = f32::from_le_bytes(buf);
        per_channel[channel_idx].push(sample);

        channel_idx += 1;
        if channel_idx == C {
            channel_idx = 0;
        }
    }

    Ok(Arc::new(per_channel)) // We return this with an Arc, as it's still a small allocation if done elsewhere
}