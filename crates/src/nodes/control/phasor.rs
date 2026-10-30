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
    /// Hz per BPM, so a `bpm` message can retune the phasor.
    hz_per_bpm: f32,
    ports: Ports,
}

impl Phasor {
    pub fn new(freq: f32) -> Self {
        Self {
            phase: 0.0,
            freq,
            hz_per_bpm: 1.0 / 60.0,
            ports: PortBuilder::default().control_out(1).build(),
        }
    }

    /// A phasor that completes one cycle every `steps` divisions of a beat.
    pub fn clock(bpm: f32, division: f32, steps: f32) -> Self {
        let hz_per_bpm = division / (60.0 * steps);
        Self {
            freq: bpm * hz_per_bpm,
            hz_per_bpm,
            ..Self::new(0.0)
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
                ("bpm", RtValue::F32(val)) => self.freq = val * self.hz_per_bpm,
                ("bpm", RtValue::U32(val)) => self.freq = val as f32 * self.hz_per_bpm,
                _ => (),
            }
        }
    }

    fn runtime_params(&self) -> Option<Vec<&str>> {
        Some(vec!["freq", "bpm"])
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

    fn create(
        _rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        let freq = p.require_f32("freq")?;
        Ok(Box::new(Self::new(freq)))
    }
}

/// Zero-size definition type for the `clock` DSL node, which derives a
/// phasor frequency from BPM, beat division, and step count.
pub struct ClockDef;

impl NodeDefinition for ClockDef {
    const NAME: &'static str = "clock";
    const DESCRIPTION: &'static str =
        "Clock signal derived from BPM, beat division, and step count. Accepts a `bpm` message";
    const REQUIRED_PARAMS: &'static [&'static str] = &["bpm", "division", "steps"];
    const OPTIONAL_PARAMS: &'static [&'static str] = &[];

    fn create(
        _rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        let bpm = p.require_f32("bpm")?;
        let division = p.require_usize("division")?;
        let steps = p.require_usize("steps")?;
        if steps == 0 {
            return Err(ValidationError::InvalidParameter(
                "clock: steps must be at least 1".into(),
            ));
        }
        Ok(Box::new(Phasor::clock(bpm, division as f32, steps as f32)))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::msg::{NodeMessage, ParamPayload};

    #[test]
    fn bpm_message_retunes_a_clock() {
        let mut clock = Phasor::clock(120.0, 4.0, 16.0);
        assert!((clock.freq - 0.5).abs() < 1e-6);

        clock.handle_msg(NodeMessage::SetParam(ParamPayload {
            param_name: "bpm",
            value: RtValue::F32(90.0),
        }));
        assert!((clock.freq - 0.375).abs() < 1e-6);
    }
}
