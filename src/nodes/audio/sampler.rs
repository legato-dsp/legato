use std::{path::Path, sync::Arc};

use arc_swap::ArcSwapOption;
use assert_no_alloc::permit_alloc;

use crate::{mini_graph::{buffer::Frame, node::{IONode}}, utils::load_sample::decode_with_ffmpeg};


pub enum AudioSampleError {
    PathNotFound,
    FailedDecoding
}

pub struct AudioSampleBackend<const C: usize> {
    data: Arc<ArcSwapOption<[Vec<f32>; C]>>
}
impl<const C: usize> AudioSampleBackend<C>{
    pub fn new( data: Arc<ArcSwapOption<[Vec<f32>; C]>>) -> Self {
        Self {
           data
        }
    }
    pub fn load_file(&self, path: &str) -> Result<(), AudioSampleError>{
        match decode_with_ffmpeg(path) {
            Ok(decoded) => {
                self.data.store(Some(decoded));
                Ok(())
            },
            Err(_) => Err(AudioSampleError::FailedDecoding) //TODO: Some logging or something?
        } 
    }
}

pub struct Sampler<const N: usize, const C: usize> {
    data: Arc<ArcSwapOption<[Vec<f32>; C]>>,
    read_pos: usize,
    is_looping: bool
}

impl<const N: usize, const C: usize> Sampler<N, C> {
    fn new(data: Arc<ArcSwapOption<[Vec<f32>; C]>>, is_looping: bool) -> Self {
        Self {
            data,
            read_pos: 0 as usize,
            is_looping
        }
    }
}

impl<const N: usize, const C: usize> IONode<N, C> for Sampler<N, C> {
    fn process(&mut self, output: &mut Frame<N, C>) {
        permit_alloc(|| { // 128 bytes allocated in the load_full. Can we do better?
            match self.data.load_full() {
                Some(buf) => {
                    let len = buf[0].len();
                    for n in 0..N {
                        let i = self.read_pos + n;
                        for c in 0..C {
                            output[c][n] = if i < len {
                                buf[c][i]
                            } else if self.is_looping {
                                buf[c][i % len]
                            } else { 0.0 };
                        }
                    }
                    self.read_pos = if self.is_looping {
                        (self.read_pos + N) % len // If we're looping, wrap around
                    } else {
                        (self.read_pos + N).min(len) // If we're not looping, cap at the end
                    };
                },
                None => ()
            }
        })
    }
}

// TODO: Can we find some minimum size so we can call with_capacity instead of growing dynamically?
pub fn build_audio_sampler<const N: usize, const C: usize>(is_looping: bool) -> (Sampler<N, C>, AudioSampleBackend<C>) {
    let data = Arc::new(ArcSwapOption::new(None));
    let sampler = Sampler::new(Arc::clone(&data), is_looping);
    let backend = AudioSampleBackend::new(data);

    (sampler, backend)
}