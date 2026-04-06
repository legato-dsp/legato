use std::{
    io::{BufReader, Read},
    process::{Command, Stdio},
    sync::Arc,
};

/// Resource buffers are designed for interacting with data
/// loaded from another thread. This could be samples, LUT, etc.
///
/// For audio, this should already be the traditional graph audio rate.
///
/// Note: These are not internally mutable. For this, you could use the
/// internal preallocated resource buffer, but you would then have to do a
/// relatively expensive copy that may not be realtime safe with large buffers.
#[derive(Clone, Default, Debug)]
pub struct ExternalBuffer {
    pub data: Arc<[f32]>,
    pub num_channels: usize,
}

impl ExternalBuffer {
    #[inline(always)]
    pub fn channel(&self, idx: usize) -> &[f32] {
        let stride = self.data.len() / self.num_channels;
        let start = idx * stride;
        &self.data[start..start + stride]
    }
}

// For the time being, we're just using FFMPEG for loading samples.
// We can do something better in the future if required, i.e streaming.
pub fn decode_with_ffmpeg(path: &str, chans: usize, sr: u32) -> std::io::Result<ExternalBuffer> {
    let mut child = Command::new("ffmpeg")
        .args([
            "-i",
            path,
            "-f",
            "f32le",
            "-ac",
            &chans.to_string(),
            "-ar",
            &sr.to_string(),
            "-acodec",
            "pcm_f32le",
            "pipe:1",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    // Collect into per-channel vecs (deinterleave ffmpeg's interleaved output)
    let mut per_channel = vec![Vec::new(); chans];
    let mut buf = [0u8; 4];
    let mut channel_idx = 0;

    while reader.read_exact(&mut buf).is_ok() {
        let sample = f32::from_le_bytes(buf);
        per_channel[channel_idx].push(sample);
        channel_idx += 1;
        if channel_idx == chans {
            channel_idx = 0;
        }
    }

    // Flatten into planar layout: [ch0_s0, ch0_s1, ..., ch1_s0, ch1_s1, ...]
    let data: Arc<[f32]> = per_channel.into_iter().flatten().collect();

    Ok(ExternalBuffer {
        data,
        num_channels: chans,
    })
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum AudioSampleError {
    PathNotFound,
    FailedDecoding,
    FrontendNotFound,
    FailedToSendToRuntime,
}
