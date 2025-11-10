use generic_array::{sequence::GenericSequence, ArrayLength, GenericArray};
use std::{
    io::{BufReader, Read},
    process::{Command, Stdio},
    sync::Arc,
};

// For the time being, we're just using FFMPEG for loading samples.
// We can do something better in the future.
pub fn decode_with_ffmpeg<C>(path: &str) -> std::io::Result<Arc<GenericArray<Vec<f32>, C>>>
where
    C: ArrayLength,
{
    let mut child = Command::new("ffmpeg")
        .args([
            "-i",
            path, // input
            "-f",
            "f32le", // correct format for f32
            "-ac",
            &C::USIZE.to_string(), // number of channels
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
    let mut per_channel = GenericArray::generate(|_| Vec::new());

    let mut buf = [0u8; 4]; // one f32 sample
    let mut channel_idx = 0;

    while reader.read_exact(&mut buf).is_ok() {
        let sample = f32::from_le_bytes(buf);
        per_channel[channel_idx].push(sample);

        channel_idx += 1;
        if channel_idx == C::USIZE {
            channel_idx = 0;
        }
    }

    Ok(Arc::new(per_channel)) // We return this with an Arc, as it's still a small allocation if done elsewhere
}
