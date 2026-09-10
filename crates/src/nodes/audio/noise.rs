use crate::{
    builder::{ResourceBuilderView, ValidationError},
    dsl::ir::DSLParams,
    node::{DynNode, Node},
    persample::PerSampleNode,
    ports::{PortBuilder, Ports},
    rng::XorShift,
    spec::NodeDefinition,
};
#[derive(Clone)]
pub struct Noise {
    rng: XorShift,
    ports: Ports,
}

impl Default for Noise {
    fn default() -> Self {
        Self::new()
    }
}

impl Noise {
    /// Spawn a new noise node.
    ///
    /// This is not some cryptographically secure noise algorithm, it aims at
    /// being fast for audio usage. The generator seeds itself from OS entropy
    /// (see [`XorShift`]) so several voices never share a stream.
    pub fn new() -> Self {
        Self::from_rng(XorShift::seeded())
    }

    pub fn with_seed(seed: u32) -> Self {
        Self::from_rng(XorShift::with_seed(seed))
    }

    fn from_rng(rng: XorShift) -> Self {
        Self {
            rng,
            ports: PortBuilder::default().audio_out(1).build(),
        }
    }

    #[inline(always)]
    pub fn white(&mut self) -> f32 {
        self.rng.bipolar()
    }
}

impl Node for Noise {
    fn ports(&self) -> &Ports {
        &self.ports
    }
    fn process(
        &mut self,
        _ctx: &mut crate::context::AudioContext,
        _inputs: &crate::node::Inputs,
        outputs: &mut [&mut [f32]],
    ) {
        if let Some(out) = outputs.get_mut(0) {
            out.iter_mut().for_each(|x| *x = self.white())
        }
    }
}

impl PerSampleNode for Noise {
    fn ports(&self) -> &Ports {
        &self.ports
    }

    fn tick(&mut self, _in_frame: &[Option<f32>], out_frame: &mut [f32]) {
        // No inputs — the generator ignores its (empty) input frame and
        // stamps a fresh white sample onto every output port.
        let sample = self.white();
        for out in out_frame.iter_mut() {
            *out = sample;
        }
    }
}

impl NodeDefinition for Noise {
    const NAME: &'static str = "noise";
    const DESCRIPTION: &'static str = "A basic noise generator";
    const REQUIRED_PARAMS: &'static [&'static str] = &[];
    const OPTIONAL_PARAMS: &'static [&'static str] = &[];

    fn create(
        _rb: &mut ResourceBuilderView,
        _p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        Ok(Box::new(Self::new()))
    }
}
