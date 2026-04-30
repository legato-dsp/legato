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

use crate::{
    builder::{ResourceBuilderView, ValidationError},
    dsl::ir::DSLParams,
    node::DynNode,
    spec::NodeDefinition,
};

impl NodeDefinition for Phasor {
    const NAME: &'static str = "phasor";
    const DESCRIPTION: &'static str = "Ramp oscillator producing a signal from 0.0 to 1.0";
    const REQUIRED_PARAMS: &'static [&'static str] = &["freq"];
    const OPTIONAL_PARAMS: &'static [&'static str] = &[];

    fn create(_rb: &mut ResourceBuilderView, p: &DSLParams) -> Result<Box<dyn DynNode>, ValidationError> {
        let freq = p.get_f32("freq").expect("Must pass frequency to phasor");
        Ok(Box::new(Self::new(freq)))
    }
}

/// Zero-size definition type for the `clock` DSL node, which derives a
/// phasor frequency from BPM, beat division, and step count.
pub struct ClockDef;

impl NodeDefinition for ClockDef {
    const NAME: &'static str = "clock";
    const DESCRIPTION: &'static str = "Clock signal derived from BPM, beat division, and step count";
    const REQUIRED_PARAMS: &'static [&'static str] = &["bpm", "division", "steps"];
    const OPTIONAL_PARAMS: &'static [&'static str] = &[];

    fn create(_rb: &mut ResourceBuilderView, p: &DSLParams) -> Result<Box<dyn DynNode>, ValidationError> {
        let bpm = p.get_usize("bpm").expect("Must pass bpm to clock");
        let division = p
            .get_usize("division")
            .expect("Must pass division to clock");
        let steps = p.get_usize("steps").expect("Must pass steps to clock");
        let freq = (bpm * division) as f32 / (60.0 * steps as f32);
        Ok(Box::new(Phasor::new(freq)))
    }
}
