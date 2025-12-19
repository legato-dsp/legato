use std::sync::{Arc, atomic::AtomicU64};

use arc_swap::ArcSwapOption;

// TODO: Should any of these just be weak pointers?

/// For now, we assume that all audio samples
/// were loaded with FFMPEG with the same rate.
///
/// However, channels can change, but we want to
/// store channels in a type-erased way, so that
/// samples can live on the audio context
#[derive(Debug, Clone)]
pub struct AudioSample {
    chans: usize,
    data: Vec<Vec<f32>>,
}

/// The audio sample handle contains a sample version
/// that lets the audio thread know if it has been updated.
///
/// This helps prevent ArcSwap loads that allocate on the
/// on the audio thread.
#[derive(Debug)]
pub struct AudioSampleHandle {
    pub sample: ArcSwapOption<AudioSample>,
    pub sample_version: AtomicU64,
}

pub struct AudioSampleRef {
    pub sample: Arc<AudioSample>,
    pub sample_version: AtomicU64,
}

impl AudioSampleHandle {
    pub fn invalidate(&self, sample: AudioSample) {
        self.sample.store(Some(Arc::new(sample)));
        self.sample_version
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

impl AudioSample {
    pub fn new(chans: usize, data: Vec<Vec<f32>>) -> Self {
        Self { chans, data }
    }
    pub fn data(&self) -> &Vec<Vec<f32>> {
        &self.data
    }
    pub fn chans(&self) -> usize {
        self.chans
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum AudioSampleError {
    PathNotFound,
    FailedDecoding,
    FrontendNotFound,
}

/// The audio sample frontend is a quick trick to load a sample
/// on another thread, which prevents interfering with the audio thread.
///
/// It's worth noting that I am not quite sure if there is UB here. It
/// may be wiser to have some sort of double buffering setup in the future,
/// or, for larger files, just having some sort of channel that streams the file
/// in, but for the time being this seems to work okay.
#[derive(Clone)]
pub struct AudioSampleFrontend {
    handle: Arc<AudioSampleHandle>,
}
impl AudioSampleFrontend {
    pub fn new(handle: Arc<AudioSampleHandle>) -> Self {
        Self { handle }
    }
    pub fn load_file(&self, path: &str, chans: usize, sr: u32) -> Result<(), AudioSampleError> {
        match decode_with_ffmpeg(path, chans, sr) {
            Ok(decoded) => {
                self.handle.invalidate(decoded);
                Ok(())
            }
            Err(_) => Err(AudioSampleError::FailedDecoding), //TODO: Some logging or something?
        }
    }
}

use std::{
    io::{BufReader, Read},
    process::{Command, Stdio},
};
// For the time being, we're just using FFMPEG for loading samples.
// We can do something better in the future if required, i.e streaming.
pub fn decode_with_ffmpeg(path: &str, chans: usize, sr: u32) -> std::io::Result<AudioSample> {
    let mut child = Command::new("ffmpeg")
        .args([
            "-i",
            path, // input
            "-f",
            "f32le", // correct format for f32
            "-ac",   // number of channels
            &chans.to_string(),
            "-ar", // sample rate
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
