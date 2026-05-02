use std::time::Duration;

use crate::{
    context::AudioContext,
    node::{Inputs, Node},
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
    pub fn new(range: &[f32], duration: Duration, chans: usize) -> Self {
        let mut new_range = [0.0; 2];
        new_range.copy_from_slice(range);
        Self {
            phase: 0.0,
            range: new_range,
            duration,
            elapsed: 0,
            ports: PortBuilder::default().audio_out(chans).build(),
        }
    }
}

impl Node for Sweep {
    fn process(&mut self, ctx: &mut AudioContext, _: &Inputs, ao: &mut [&mut [f32]]) {
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

use crate::{
    builder::{ResourceBuilderView, ValidationError},
    dsl::ir::DSLParams,
    node::DynNode,
    spec::NodeDefinition,
};

impl NodeDefinition for Sweep {
    const NAME: &'static str = "sweep";
    const DESCRIPTION: &'static str =
        "Frequency sweep oscillator over a configurable range and duration";
    const REQUIRED_PARAMS: &'static [&'static str] = &[];
    const OPTIONAL_PARAMS: &'static [&'static str] = &["duration", "range", "chans"];

    fn create(
        _rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        use std::time::Duration;
        let chans = p.get_usize("chans").unwrap_or(2);
        let duration = p
            .get_duration_ms("duration")
            .unwrap_or(Duration::from_secs_f32(5.0));
        let range = p.get_array_f32("range").unwrap_or([40., 48_000.].into());
        Ok(Box::new(Self::new(&range, duration, chans)))
    }
}
