use crate::{
    context::AudioContext,
    node::{Inputs, Node},
    persample::PerSampleNode,
    ports::{PortBuilder, Ports},
};

/// The Hadamard mixer applies a fast Walsh-Hadamard
/// transform (FWHT).
///
/// https://en.wikipedia.org/wiki/Fast_Walsh%E2%80%93Hadamard_transform
///
/// These mixers are generally good at creating more
/// density in FDN.
///
/// `chans` must be a power of two or it will panic!
#[derive(Clone)]
pub struct HadamardMixer {
    ports: Ports,
    chans: usize,
    vertical_slice: Box<[f32]>,
}

impl HadamardMixer {
    pub fn new(chans: usize) -> Self {
        assert!(chans.is_power_of_two());
        Self {
            ports: PortBuilder::default()
                .audio_in(chans)
                .audio_out(chans)
                .build(),
            chans,
            vertical_slice: vec![0.0; chans].into(), // could maybe be an enum and on the stack?
        }
    }

    /// Update the FWHT in place
    ///
    /// see: https://en.wikipedia.org/wiki/Fast_Walsh%E2%80%93Hadamard_transform
    fn fht(a: &mut [f32]) {
        let n = a.len();
        let mut h = 1;
        while h < n {
            let mut i = 0;
            while i < n {
                for j in i..i + h {
                    let x = a[j];
                    let y = a[j + h];
                    a[j] = x + y;
                    a[j + h] = x - y;
                }
                i += h * 2;
            }
            h *= 2;
        }
        // Normalize
        let norm = 1.0 / (n as f32).sqrt();
        a.iter_mut().for_each(|x| *x *= norm);
    }
}

impl Node for HadamardMixer {
    fn process(&mut self, ctx: &mut AudioContext, inputs: &Inputs, outputs: &mut [&mut [f32]]) {
        let block_size = ctx.get_config().block_size;

        for i in 0..block_size {
            for c in 0..self.chans {
                self.vertical_slice[c] = inputs.get(c).and_then(|x| *x).map_or(0.0, |buf| buf[i]);
            }
            Self::fht(&mut self.vertical_slice); // apply transform
            for c in 0..self.chans {
                outputs[c][i] = self.vertical_slice[c];
            }
        }
    }
    fn ports(&self) -> &Ports {
        &self.ports
    }
}

impl PerSampleNode for HadamardMixer {
    fn ports(&self) -> &Ports {
        &self.ports
    }

    fn tick(&mut self, in_frame: &[Option<f32>], out_frame: &mut [f32]) {
        for c in 0..self.chans {
            self.vertical_slice[c] = in_frame[c].unwrap_or(0.0);
        }
        Self::fht(&mut self.vertical_slice); // apply transform
        for c in 0..self.chans {
            out_frame[c] = self.vertical_slice[c];
        }
    }
}

use crate::{
    builder::{ResourceBuilderView, ValidationError},
    dsl::ir::DSLParams,
    node::DynNode,
    spec::NodeDefinition,
};

impl HadamardMixer {
    pub fn from_params(
        _rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Self, ValidationError> {
        let chans = p
            .get_usize("chans")
            .expect("Must provide chans to hadamard");
        Ok(Self::new(chans))
    }
}

impl NodeDefinition for HadamardMixer {
    const NAME: &'static str = "hadamard";
    const DESCRIPTION: &'static str = "Walsh-Hadamard transform mixer for feedback delay networks";
    const REQUIRED_PARAMS: &'static [&'static str] = &["chans"];
    const OPTIONAL_PARAMS: &'static [&'static str] = &[];

    fn create(
        rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        Ok(Box::new(Self::from_params(rb, p)?))
    }
}
