use std::time::Duration;

use crate::{
    context::AudioContext,
    node::{Channels, Inputs, Node},
    ports::{PortBuilder, Ports},
};

#[derive(Clone)]
pub struct Sweep {
    phase: f32,
    range: [f32; 2],
    duration: Duration,
    elapsed: usize,
    ports: Ports,
}

impl Sweep {
    pub fn new(range: [f32; 2], duration: Duration, chans: usize) -> Self {
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
    fn process(&mut self, ctx: &mut AudioContext, _: &Inputs, ao: &mut Channels) {
        let config = ctx.get_config();

        let fs = config.sample_rate as f32;

        let block_size = ctx.get_config().block_size;

        let mut min = self.range[0];
        let max = self.range[1];

        min = min.clamp(1.0, max);

        for n in 0..block_size {
            let t = (self.elapsed as f32 / fs).min(self.duration.as_secs_f32());
            let freq = min * ((max / min).powf(t / self.duration.as_secs_f32()));
            self.elapsed += 1;

            self.phase += freq / fs;
            self.phase = self.phase.fract();

            let sample = (self.phase * std::f32::consts::TAU).sin();

            for chan in ao.iter_mut() {
                chan[n] = sample;
            }
        }
    }
    fn ports(&self) -> &Ports {
        &self.ports
    }
}
