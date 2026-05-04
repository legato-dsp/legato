use crate::{
    context::AudioContext,
    node::{Inputs, Node},
    ports::{PortBuilder, Ports},
    resources::params::ParamKey,
};

#[derive(Clone)]
pub struct Signal {
    key: ParamKey,
    val: f32,
    smoothing: f32,
    ports: Ports,
}

impl Signal {
    pub fn new(key: ParamKey, val: f32, smoothing_factor: f32) -> Self {
        Self {
            key,
            val,
            smoothing: smoothing_factor,
            ports: PortBuilder::default().audio_out(1).build(),
        }
    }
}

impl Node for Signal {
    fn process(&mut self, ctx: &mut AudioContext, _: &Inputs, outputs: &mut [&mut [f32]]) {
        // Param set on each block, then smoothed with a one pole filter
        // Maybe we do this per control sample as well in the future with less smoothing, provided the benchmark is decent
        if let Ok(target) = ctx.get_param(&self.key) {
            for channel in outputs.iter_mut() {
                for sample in channel.iter_mut() {
                    self.val += (target - self.val) * self.smoothing;
                    *sample = self.val;
                }
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
    resources::params::ParamMeta,
    spec::NodeDefinition,
};

impl NodeDefinition for Signal {
    const NAME: &'static str = "signal";
    const DESCRIPTION: &'static str = "Smoothed control parameter exposed to the host application";
    const REQUIRED_PARAMS: &'static [&'static str] = &["name", "min", "max", "default"];
    const OPTIONAL_PARAMS: &'static [&'static str] = &["smoothing"];

    fn create(
        rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        let name = p.get_str("name").expect("Must pass name to signal!");
        let min = p.get_f32("min").expect("Must provide min to signal!");
        let max = p.get_f32("max").expect("Must provide max to signal!");
        let default = p
            .get_f32("default")
            .expect("Must provide default(f32) to signal!");
        let smoothing = p.get_f32("smoothing").unwrap_or(0.5).clamp(0.0, 1.0);
        let meta = ParamMeta {
            name: name.clone(),
            min,
            max,
            default,
        };
        let key = rb.add_param(name, meta);
        Ok(Box::new(Self::new(key, default, smoothing)))
    }
}
