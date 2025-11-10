use std::sync::Arc;

use arc_swap::ArcSwapOption;
use assert_no_alloc::permit_alloc;
use generic_array::{ArrayLength, GenericArray};
use typenum::U0;

use crate::{
    engine::{
        audio_context::AudioContext,
        buffer::Frame,
        node::{FrameSize, Node},
        port::{Stereo, *},
    },
    nodes::utils::{ffmpeg::decode_with_ffmpeg, port_utils::generate_audio_outputs},
};

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum AudioSampleError {
    PathNotFound,
    FailedDecoding,
}

pub struct AudioSampleBackend<C>
where
    C: ArrayLength,
{
    data: Arc<ArcSwapOption<GenericArray<Vec<f32>, C>>>,
}
impl<C> AudioSampleBackend<C>
where
    C: ArrayLength,
{
    pub fn new(data: Arc<ArcSwapOption<GenericArray<Vec<f32>, C>>>) -> Self {
        Self { data }
    }
    pub fn load_file(&self, path: &str) -> Result<(), AudioSampleError> {
        match decode_with_ffmpeg(path) {
            Ok(decoded) => {
                self.data.store(Some(decoded));
                Ok(())
            }
            Err(_) => Err(AudioSampleError::FailedDecoding), //TODO: Some logging or something?
        }
    }
}

// TODO: This is lazy, maybe integrate with symponia crate or whatever it's called?
pub struct Sampler<Ao>
where
    Ao: ArrayLength,
{
    data: Arc<ArcSwapOption<GenericArray<Vec<f32>, Ao>>>,
    read_pos: usize,
    is_looping: bool,
    ports: Ports<U0, Ao, U0, U0>,
}

impl<Ao> Sampler<Ao>
where
    Ao: ArrayLength,
{
    pub fn new(data: Arc<ArcSwapOption<GenericArray<Vec<f32>, Ao>>>) -> Self {
        Self {
            data,
            read_pos: 0,
            is_looping: true,
            ports: Ports {
                audio_inputs: None,
                audio_outputs: Some(generate_audio_outputs()),
                control_inputs: None, // TODO, Trig, Volume, etc.
                control_outputs: None,
            },
        }
    }
}

impl<AF, CF, Ao> Node<AF, CF> for Sampler<Ao>
where
    AF: FrameSize,
    CF: FrameSize,
    Ao: ArrayLength,
{
    fn process(
        &mut self,
        _: &mut AudioContext<AF>,
        _: &Frame<AF>,
        ao: &mut Frame<AF>,
        _: &Frame<CF>,
        _: &mut Frame<CF>,
    ) {
        permit_alloc(|| {
            // 128 bytes allocated in the load_full. Can we do better?
            if let Some(buf) = self.data.load_full() {
                let len = buf[0].len();
                for n in 0..AF::USIZE {
                    let i = self.read_pos + n;
                    for c in 0..Ao::USIZE {
                        ao[c][n] = if i < len {
                            buf[c][i]
                        } else if self.is_looping {
                            buf[c][i % len]
                        } else {
                            0.0
                        };
                    }
                }
                self.read_pos = if self.is_looping {
                    (self.read_pos + AF::USIZE) % len // If we're looping, wrap around
                } else {
                    (self.read_pos + AF::USIZE).min(len) // If we're not looping, cap at the end
                };
            }
        })
    }
}
impl<Ao> PortedErased for Sampler<Ao>
where
    Ao: ArrayLength,
{
    fn get_audio_inputs(&self) -> Option<&[AudioInputPort]> {
        self.ports.get_audio_inputs()
    }
    fn get_audio_outputs(&self) -> Option<&[AudioOutputPort]> {
        self.ports.get_audio_outputs()
    }
    fn get_control_inputs(&self) -> Option<&[ControlInputPort]> {
        self.ports.get_control_inputs()
    }
    fn get_control_outputs(&self) -> Option<&[ControlOutputPort]> {
        self.ports.get_control_outputs()
    }
}

pub type SamplerMono<const AF: usize> = Sampler<Mono>;
pub type SamplerStereo<const AF: usize> = Sampler<Stereo>;
