use crate::{
    context::AudioContext,
    node::{Inputs, Node},
    persample::PerSampleNode,
    ports::{PortBuilder, Ports},
};

/// As suggested in https://signalsmith-audio.co.uk/writing/2021/lets-write-a-reverb/
///
/// Allegedly a bit lower density than saw a hadamard mixer
#[derive(Clone)]
pub struct HouseholderMixer {
    chans: usize,
    ports: Ports,
}

impl HouseholderMixer {
    pub fn new(chans: usize) -> Self {
        Self {
            chans,
            ports: PortBuilder::default()
                .audio_in(chans)
                .audio_out(chans)
                .build(),
        }
    }
}

impl Node for HouseholderMixer {
    fn process(&mut self, _ctx: &mut AudioContext, inputs: &Inputs, outputs: &mut [&mut [f32]]) {
        let block_size = outputs[0].len();
        let multiplier = 2.0 / self.chans as f32;

        for i in 0..block_size {
            let sum: f32 = (0..self.chans)
                .map(|c| inputs.get(c).and_then(|x| *x).map_or(0.0, |buf| buf[i]))
                .sum();
            for c in 0..self.chans {
                let x = inputs.get(c).and_then(|x| *x).map_or(0.0, |buf| buf[i]);
                outputs[c][i] = x - multiplier * sum;
            }
        }
    }
    fn ports(&self) -> &Ports {
        &self.ports
    }
}

impl PerSampleNode for HouseholderMixer {
    fn ports(&self) -> &Ports {
        &self.ports
    }

    fn tick(&mut self, in_frame: &[Option<f32>], out_frame: &mut [f32]) {
        let multiplier = 2.0 / self.chans as f32;
        let sum: f32 = (0..self.chans).map(|c| in_frame[c].unwrap_or(0.0)).sum();
        for c in 0..self.chans {
            let x = in_frame[c].unwrap_or(0.0);
            out_frame[c] = x - multiplier * sum;
        }
    }
}

use crate::{
    builder::{ResourceBuilderView, ValidationError},
    dsl::ir::DSLParams,
    node::DynNode,
    spec::NodeDefinition,
};

impl HouseholderMixer {
    pub fn from_params(
        _rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Self, ValidationError> {
        let chans = p
            .get_usize("chans")
            .expect("Must provide chans to householder");
        Ok(Self::new(chans))
    }
}

impl NodeDefinition for HouseholderMixer {
    const NAME: &'static str = "householder";
    const DESCRIPTION: &'static str = "Householder reflection mixer for feedback delay networks";
    const REQUIRED_PARAMS: &'static [&'static str] = &["chans"];
    const OPTIONAL_PARAMS: &'static [&'static str] = &[];

    fn create(
        rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        Ok(Box::new(Self::from_params(rb, p)?))
    }
}
