use assert_no_alloc::permit_alloc;
use generic_array::ArrayLength;
use typenum::U0;

use crate::{
    engine::{
        audio_context::AudioContext,
        buffer::Frame,
        node::{FrameSize, Node},
        port::{Stereo, *},
        resources::SampleKey,
    },
    nodes::utils::port_utils::generate_audio_outputs,
};

pub struct Sampler<Ao>
where
    Ao: ArrayLength,
{
    sample_key: SampleKey,
    read_pos: usize,
    is_looping: bool,
    ports: Ports<U0, Ao, U0, U0>,
}

impl<Ao> Sampler<Ao>
where
    Ao: ArrayLength,
{
    pub fn new(sample_key: SampleKey) -> Self {
        Self {
            sample_key,
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
        ctx: &mut AudioContext<AF>,
        _: &Frame<AF>,
        ao: &mut Frame<AF>,
        _: &Frame<CF>,
        _: &mut Frame<CF>,
    ) {
        permit_alloc(|| {
            // 128 bytes allocated in the load_full. Can we do better?
            if let Some(inner) = ctx.get_sample(self.sample_key) {
                let buf = inner.data();
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
