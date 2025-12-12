use std::time::Duration;

use crate::{
    context::AudioContext,
    node::{Channels, Node},
    ports::{PortBuilder, Ported, Ports},
};

pub struct Sweep {
    phase: f32,
    range: (f32, f32),
    duration: Duration,
    elapsed: usize,
    ports: Ports,
}

impl Sweep {
    pub fn new(range: (f32, f32), duration: Duration, chans: usize) -> Self {
        Self {
            phase: 0.0,
            range,
            duration,
            elapsed: 0,
            ports: PortBuilder::default().audio_out(chans).build(),
        }
    }
}

impl Node for Sweep {
    fn process(
        &mut self,
        ctx: &mut AudioContext,
        _: &Channels,
        ao: &mut Channels,
        _: &Channels,
        _: &mut Channels,
    ) {
        let config = ctx.get_config();

        let fs = config.sample_rate as f32;

        let block_size = ctx.get_config().audio_block_size;

        let chans = ao.len();

        let (mut min, max) = self.range;

        min = min.clamp(1.0, max);

        for n in 0..block_size {
            let t = (self.elapsed as f32 / fs).min(self.duration.as_secs_f32());
            let freq = min * ((max / min).powf(t / self.duration.as_secs_f32()));
            self.elapsed += 1;

            self.phase += freq / fs;
            self.phase = self.phase.fract();

            let sample = (self.phase * std::f32::consts::TAU).sin();

            for c in 0..chans {
                ao[c][n] = sample;
            }
        }
    }
    fn ports(&self) -> &Ports {
        &self.ports
    }
}
