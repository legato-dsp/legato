use crate::{context::AudioContext, node::{Channels, Node}, params::ParamKey, ports::{PortBuilder, Ports}};

#[derive(Clone)]
pub struct ControlSignal {
    key: ParamKey,
    val: f32,
    smoothing: f32,
    ports: Ports
}

impl ControlSignal {
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

impl Node for ControlSignal {
    fn process(&mut self, ctx: &mut AudioContext, _: &Channels,_: &mut Channels, _: &Channels, co: &mut Channels){
        // Param set on each block, then smoothed with a one pole filter
        // Maybe we do this per control sample as well in the future with less smoothing, provided the benchmark is decent
        if let Ok(target) = ctx.get_param(&self.key){
            for channel in co.iter_mut() {
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