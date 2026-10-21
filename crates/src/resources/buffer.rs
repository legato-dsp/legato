use std::{
    io::ErrorKind,
    path::Path,
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

    /// Get the length of samples for the current sample.
    ///
    /// This assumes some symmetry between the channels, but is fine for now
    #[inline(always)]
    pub fn len_samples(&self) -> usize {
        self.data.len() / self.num_channels
    }
}

// For the time being, we're just using FFMPEG for loading samples.
// We can do something better in the future if required, i.e streaming.
//
// Requires an `ffmpeg` binary on the PATH.
pub fn decode_with_ffmpeg(
    path: &Path,
    chans: usize,
    sr: u32,
) -> Result<ExternalBuffer, AudioSampleError> {
    if chans == 0 {
        return Err(AudioSampleError::FailedDecoding(
            "chans must be at least 1".into(),
        ));
    }
    if !path.is_file() {
        return Err(AudioSampleError::PathNotFound(path.display().to_string()));
    }

    let output = Command::new("ffmpeg")
        .arg("-i")
        .arg(path)
        .args([
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
        .stdin(Stdio::null())
        .output()
        .map_err(|e| match e.kind() {
            ErrorKind::NotFound => AudioSampleError::FfmpegNotFound,
            _ => AudioSampleError::FailedDecoding(e.to_string()),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let reason = stderr.lines().last().unwrap_or("ffmpeg failed").to_string();
        return Err(AudioSampleError::FailedDecoding(reason));
    }

    let frames = output.stdout.len() / (4 * chans);
    if frames == 0 {
        return Err(AudioSampleError::FailedDecoding(format!(
            "{} decoded to no audio",
            path.display()
        )));
    }

    // Deinterleave ffmpeg's output into a planar layout: [ch0_s0, ch0_s1, ..., ch1_s0, ...]
    let (bytes, _) = output.stdout[..frames * chans * 4].as_chunks::<4>();
    let samples: Vec<f32> = bytes.iter().map(|b| f32::from_le_bytes(*b)).collect();
    let data: Arc<[f32]> = (0..chans)
        .flat_map(|c| samples.iter().skip(c).step_by(chans).copied())
        .collect();

    Ok(ExternalBuffer {
        data,
        num_channels: chans,
    })
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum AudioSampleError {
    #[error("sample file not found: {0}")]
    PathNotFound(String),
    /// No `ffmpeg` binary on the PATH; it is needed to decode samples.
    #[error("ffmpeg not found on PATH; it is required to load samples")]
    FfmpegNotFound,
    #[error("failed to decode sample: {0}")]
    FailedDecoding(String),
    /// No sampler or grain node declared this buffer name.
    #[error("no sample buffer named '{0}' in the graph")]
    BufferNotFound(String),
    #[error("frontend not found")]
    FrontendNotFound,
    #[error("the runtime's sample queue is full")]
    FailedToSendToRuntime,
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn missing_file_is_path_not_found() {
        let err = decode_with_ffmpeg(Path::new("does/not/exist.wav"), 2, 48_000).unwrap_err();
        assert!(matches!(err, AudioSampleError::PathNotFound(_)), "{err:?}");
    }
}
