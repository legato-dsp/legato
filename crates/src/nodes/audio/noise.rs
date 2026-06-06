use crate::{
    builder::{ResourceBuilderView, ValidationError},
    dsl::ir::DSLParams,
    node::{DynNode, Node},
    ports::{PortBuilder, Ports},
    spec::NodeDefinition,
};

#[derive(Clone)]
pub struct Noise {
    state: u32,
    ports: Ports,
}

impl Default for Noise {
    fn default() -> Self {
        Self::new()
    }
}

impl Noise {
    pub fn new() -> Self {
        Self {
            state: 0xBAADF00D,
            ports: PortBuilder::default().audio_out(1).build(),
        }
    }

    // Yields the next pseudo-random u32 val
    #[inline(always)]
    fn next_val(&mut self) -> u32 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 17;
        self.state ^= self.state << 5;
        self.state
    }

    #[inline(always)]
    pub fn white(&mut self) -> f32 {
        // Map u32 to -1,1
        // TODO: Is there something with less ops?
        // TODO: Could I get subnormals here?
        (self.next_val() as i32 as f32) * (1.0 / i32::MAX as f32)
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
