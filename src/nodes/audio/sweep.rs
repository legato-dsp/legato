use std::time::Duration;

use typenum::{U0, U1};

use crate::{
    engine::{
        audio_context::AudioContext,
        buffer::Frame,
        node::{FrameSize, Node},
        port::{
            AudioInputPort, AudioOutputPort, ControlInputPort, ControlOutputPort, PortedErased,
            Ports,
        },
    },
    nodes::utils::port_utils::generate_audio_outputs,
};

pub struct Sweep {
    phase: f32,
    range: (f32, f32),
    duration: Duration,
    elapsed: usize,
    ports: Ports<U0, U1, U0, U0>,
}

impl Sweep {
    pub fn new(range: (f32, f32), duration: Duration) -> Self {
        Self {
            phase: 0.0,
            range,
            duration,
            elapsed: 0,
            ports: Ports {
                audio_inputs: None,
                audio_outputs: Some(generate_audio_outputs()),
                control_inputs: None,
                control_outputs: None,
            },
        }
    }
}

impl<AF, CF> Node<AF, CF> for Sweep
where
    AF: FrameSize,
    CF: FrameSize,
{
    fn process(
        &mut self,
        ctx: &mut AudioContext<AF>,
        _: &Frame<AF>,
        ao: &mut Frame<AF>,
        _: &Frame<CF>,
        _: &mut Frame<CF>,
    ) {
        let fs = ctx.get_sample_rate();

        let (min, max) = self.range;

        for n in 0..AF::USIZE {
            let t = (self.elapsed as f32 / fs).min(self.duration.as_secs_f32());
            let freq = min * ((max / min).powf(t / self.duration.as_secs_f32()));
            self.elapsed += 1;

            self.phase += freq / fs;
            self.phase = self.phase.fract();

            let sample = (self.phase * std::f32::consts::TAU).sin();

            ao[0][n] = sample;
        }
    }
}

impl PortedErased for Sweep {
    fn get_audio_inputs(&self) -> Option<&[AudioInputPort]> {
        None
    }
    fn get_audio_outputs(&self) -> Option<&[AudioOutputPort]> {
        self.ports.get_audio_outputs()
    }
    fn get_control_inputs(&self) -> Option<&[ControlInputPort]> {
        None
    }
    fn get_control_outputs(&self) -> Option<&[ControlOutputPort]> {
        None
    }
}
