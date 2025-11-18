use crate::engine::audio_context::AudioContext;
use crate::engine::node::FrameSize;
use crate::engine::port::{
    AudioInputPort, AudioOutputPort, ControlInputPort, ControlOutputPort, PortMeta,
};
use crate::engine::{buffer::Frame, node::Node, port::PortedErased};

pub struct Stereo {
    ports: StereoPorts,
}

impl Default for Stereo {
    fn default() -> Self {
        Self {
            ports: StereoPorts::new(),
        }
    }
}

impl<AF, CF> Node<AF, CF> for Stereo
where
    AF: FrameSize,
    CF: FrameSize,
{
    fn process(
        &mut self,
        _: &mut AudioContext<AF>,
        ai: &Frame<AF>,
        ao: &mut Frame<AF>,
        _: &Frame<CF>,
        _: &mut Frame<CF>,
    ) {
        debug_assert_eq!(ai.len(), 1);
        debug_assert_eq!(ao.len(), 2);

        for n in 0..AF::USIZE {
            for c in 0..2 {
                ao[c][n] = ai[0][n];
            }
        }
    }
}

struct StereoPorts {
    audio_inputs: [AudioInputPort; 1],
    audio_outputs: [AudioOutputPort; 2],
}
impl StereoPorts {
    pub fn new() -> Self {
        Self {
            audio_inputs: [AudioInputPort {
                meta: PortMeta {
                    name: "audio",
                    index: 0,
                },
            }],
            audio_outputs: [
                AudioOutputPort {
                    meta: PortMeta {
                        name: "l",
                        index: 0,
                    },
                },
                AudioOutputPort {
                    meta: PortMeta {
                        name: "r",
                        index: 1,
                    },
                },
            ],
        }
    }
}

impl PortedErased for Stereo {
    fn get_audio_inputs(&self) -> Option<&[AudioInputPort]> {
        Some(&self.ports.audio_inputs)
    }
    fn get_audio_outputs(&self) -> Option<&[AudioOutputPort]> {
        Some(&self.ports.audio_outputs)
    }
    fn get_control_inputs(&self) -> Option<&[ControlInputPort]> {
        None
    }
    fn get_control_outputs(&self) -> Option<&[ControlOutputPort]> {
        None
    }
}
