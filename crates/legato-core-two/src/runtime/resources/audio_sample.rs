use std::sync::Arc;

use arc_swap::ArcSwapOption;

/// For now, we assume that all audio samples
/// were loaded with FFMPEG with the same rate.
///
/// However, channels can change, but we want to
/// store channels in a type-erased way, so that
/// samples can live on the audio context
pub struct AudioSample {
    chans: usize,
    data: Vec<Vec<f32>>,
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
    BackendNotFound
}

/// The audio sample backend is a quick trick to load a sample
/// on another thread, which prevents interfering with the audio thread.
///
/// It's worth noting that I am not quite sure if there is UB here. It
/// may be wiser to have some sort of double buffering setup in the future,
/// or, for larger files, just having some sort of channel that streams the file
/// in, but for the time being this seems to work okay.
#[derive(Clone)]
pub struct AudioSampleBackend {
    data: Arc<ArcSwapOption<AudioSample>>,
}
impl AudioSampleBackend {
    pub fn new(data: Arc<ArcSwapOption<AudioSample>>) -> Self {
        Self { data }
    }
    pub fn load_file(&self, path: &str, chans: usize, sr: u32) -> Result<(), AudioSampleError> {
        match decode_with_ffmpeg(path, chans, sr) {
            Ok(decoded) => {
                self.data.store(Some(Arc::new(decoded)));
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
            path,               // input
            "-f",
            "f32le",            // correct format for f32
            "-ac",              // number of channels
            &chans.to_string(), 
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

    println!("{}", per_channel[0].len());

    Ok(AudioSample::new(chans, per_channel))
}
