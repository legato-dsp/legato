use crate::{context::AudioContext, node::{Channels, Node}, params::ParamKey, ports::{PortBuilder, Ports}};

#[derive(Clone)]
pub struct Signal {
    key: ParamKey,
    val: f32,
    smoothing: f32,
    ports: Ports
}

impl Signal {
    pub fn new(key: ParamKey, val: f32, smoothing_factor: f32) -> Self {
        Self {
            key,
            val,
             smoothing: smoothing_factor,
             ports: PortBuilder::default()
                .control_out(1)
                .build()
        }
    }
}

impl Node for Signal {
    fn process(&mut self, ctx: &mut AudioContext, _: &Channels, outputs: &mut Channels){
        // Param set on each block, then smoothed with a one pole filter
        // Maybe we do this per control sample as well in the future with less smoothing, provided the benchmark is decent
        if let Ok(target) = ctx.get_param(&self.key){
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