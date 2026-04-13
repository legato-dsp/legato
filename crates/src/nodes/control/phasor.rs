use crate::{
    context::AudioContext,
    msg::{self, RtValue},
    node::{Inputs, Node},
    ports::{PortBuilder, Ports},
};

#[derive(Clone, Debug)]
pub struct Phasor {
    phase: f32,
    freq: f32,
    ports: Ports,
}

impl Phasor {
    pub fn new(freq: f32) -> Self {
        Self {
            phase: 0.0,
            freq,
            ports: PortBuilder::default().audio_out(1).build(),
        }
    }

    #[inline(always)]
    fn tick(&mut self, inc: f32) -> f32 {
        self.phase += inc;

        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }

        self.phase
    }
}

impl Node for Phasor {
    fn process(&mut self, ctx: &mut AudioContext, _: &Inputs, outputs: &mut [&mut [f32]]) {
        let fs_recipricol = 1.0 / ctx.get_config().sample_rate as f32;
        let inc = self.freq * fs_recipricol;

        outputs
            .get_mut(0)
            .unwrap()
            .iter_mut()
            .for_each(|x| *x = self.tick(inc));
    }

    fn ports(&self) -> &Ports {
        &self.ports
    }

    fn handle_msg(&mut self, msg: crate::msg::NodeMessage) {
        if let msg::NodeMessage::SetParam(inner) = msg {
            match (inner.param_name, inner.value) {
                ("freq", RtValue::F32(val)) => self.freq = val,
                ("freq", RtValue::U32(val)) => self.freq = val as f32,
                _ => (),
            }
        }
    }
}
