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
            ports: PortBuilder::default().control_out(1).build(),
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

use std::time::Duration;

use crate::{
    builder::{ResourceBuilderView, ValidationError},
    dsl::ir::DSLParams,
    node::DynNode,
    node_state::FADE_MS,
    resources::params::ParamMeta,
    spec::NodeDefinition,
};

/// One-pole coefficient that covers 1 - 1/e of a step in `glide`, the same
/// time constant at any sample rate; 1.0 (no glide) under one sample.
pub fn glide_coefficient(glide: Duration, sample_rate: usize) -> f32 {
    let samples = glide.as_secs_f64() * sample_rate as f64;
    if samples < 1.0 {
        1.0
    } else {
        (1.0 - (-1.0 / samples).exp()) as f32
    }
}

impl NodeDefinition for Signal {
    const NAME: &'static str = "signal";
    const DESCRIPTION: &'static str = "Control parameter exposed to the host application, gliding to each new value over `glide` ms (default 5, 0 for none)";
    const REQUIRED_PARAMS: &'static [&'static str] = &["name", "min", "max", "default"];
    const OPTIONAL_PARAMS: &'static [&'static str] = &["glide"];

    fn create(
        rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        let name = p.require_str("name")?;
        let min = p.require_f32("min")?;
        let max = p.require_f32("max")?;
        let default = p.require_f32("default")?;
        let glide = p
            .get_duration_ms("glide")?
            .unwrap_or(Duration::from_millis(FADE_MS as u64));
        let smoothing = glide_coefficient(glide, rb.config.sample_rate);
        let meta = ParamMeta {
            name: name.clone(),
            min,
            max,
            default,
        };
        let key = rb.add_param(name, meta)?;
        Ok(Box::new(Self::new(key, default, smoothing)))
    }
}
