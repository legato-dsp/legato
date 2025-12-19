use crate::{context::AudioContext, node::{Channels, Node}, params::ParamKey, ports::{PortBuilder, Ports}};

pub struct AudioSignal {
    key: ParamKey,
    val: f32,
    smoothing_factor: f32,
    ports: Ports
}

impl AudioSignal {
    pub fn new(key: ParamKey, val: f32, smoothing_factor: f32) -> Self {
        Self {
            key,
            val,
             smoothing_factor,
             ports: PortBuilder::default()
                .audio_out(1)
                .build()
        }
    }
}

impl Node for AudioSignal {
    fn process(&mut self, ctx: &mut AudioContext, _: &Channels, ao: &mut Channels, _: &Channels, _: &mut Channels){
        // Param set on each block, then smoothed with a one pole filter
        // Maybe we do this per control sample as well in the future with less smoothing, provided the benchmark is decent
        if let Ok(target) = ctx.get_param(&self.key){
            for channel in ao.iter_mut() {
                for sample in channel.iter_mut() {
                    self.val += (target - self.val) * self.smoothing_factor;
                    *sample = self.val;
                }
            }
        }
    }
    fn ports(&self) -> &Ports {
        &self.ports
    }
}