use std::sync::Arc;

use arc_swap::ArcSwapOption;

use crate::nodes::utils::ffmpeg::decode_with_ffmpeg;

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
