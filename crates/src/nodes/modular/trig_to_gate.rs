use std::time::Duration;

use crate::{
    builder::{ResourceBuilderView, ValidationError},
    context::AudioContext,
    dsl::ir::DSLParams,
    node::{DynNode, Inputs, Node},
    persample::PerSampleNode,
    ports::{PortBuilder, Ports},
    spec::NodeDefinition,
};

#[derive(Clone)]
pub struct TrigToGate {
    gate_time_in_samples: u32,
    samples_left: u32,
    last_trig: f32,
    ports: Ports,
}

impl TrigToGate {
    pub fn new(gate_time_in_samples: u32) -> Self {
        Self {
            gate_time_in_samples: gate_time_in_samples.max(1),
            samples_left: 0,
            last_trig: 0.0,
            ports: PortBuilder::default()
                .control_in_named(&["trig"])
                .default_in()
                .control_out_named(&["gate"])
                .default_out()
                .build(),
        }
    }

    #[inline(always)]
    fn tick_inner(&mut self, trig: f32) -> f32 {
        let rising_edge = trig > 0.5 && self.last_trig <= 0.5;

        self.last_trig = trig;

        if rising_edge {
            self.samples_left = self.gate_time_in_samples;
        }

        if self.samples_left > 0 {
            self.samples_left -= 1;
            1.0
        } else {
            0.0
        }
    }
}

impl TrigToGate {
    pub fn from_params(
        rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Self, ValidationError> {
        let sr = rb.config.sample_rate as f32;
        let gate_time = p
            .get_duration_ms("gate_time")
            .unwrap_or(Duration::from_millis(100));

        Ok(Self::new((gate_time.as_secs_f32() * sr).floor() as u32))
    }
}

impl NodeDefinition for TrigToGate {
    const NAME: &'static str = "trig_to_gate";
    const DESCRIPTION: &'static str = "Holds a gate high for `gate_time` ms on every rising edge of `trig`, so a trigger source such as `quantize.trig` can drive an envelope.";
    const REQUIRED_PARAMS: &'static [&'static str] = &[];
    const OPTIONAL_PARAMS: &'static [&'static str] = &["gate_time"];

    fn create(
        rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        Ok(Box::new(Self::from_params(rb, p)?))
    }
}

impl Node for TrigToGate {
    fn process(&mut self, _ctx: &mut AudioContext, inputs: &Inputs, outputs: &mut [&mut [f32]]) {
        let Some(trig_in) = inputs[0] else { return };

        for (trig, gate) in trig_in.iter().zip(outputs[0].iter_mut()) {
            *gate = self.tick_inner(*trig);
        }
    }

    fn ports(&self) -> &Ports {
        &self.ports
    }
}

impl PerSampleNode for TrigToGate {
    fn ports(&self) -> &Ports {
        &self.ports
    }

    fn tick(&mut self, in_frame: &[Option<f32>], out_frame: &mut [f32]) {
        out_frame[0] = self.tick_inner(in_frame[0].unwrap_or(0.0));
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn gates(node: &mut TrigToGate, trigs: &[f32]) -> Vec<f32> {
        let mut out = [0.0f32; 1];
        trigs
            .iter()
            .map(|t| {
                PerSampleNode::tick(node, &[Some(*t)], &mut out);
                out[0]
            })
            .collect()
    }

    #[test]
    fn one_sample_trigger_becomes_a_held_gate() {
        let mut node = TrigToGate::new(3);
        assert_eq!(
            gates(&mut node, &[0.0, 1.0, 0.0, 0.0, 0.0, 0.0]),
            vec![0.0, 1.0, 1.0, 1.0, 0.0, 0.0]
        );
    }

    #[test]
    fn gate_is_exactly_one_and_zero() {
        let mut node = TrigToGate::new(2);
        for gate in gates(&mut node, &[1.0, 0.0, 0.0, 0.0]) {
            assert!(gate == 1.0 || gate == 0.0, "adsr compares gate exactly");
        }
    }

    #[test]
    fn a_held_trigger_does_not_extend_the_gate() {
        let mut node = TrigToGate::new(2);
        assert_eq!(
            gates(&mut node, &[1.0, 1.0, 1.0, 1.0]),
            vec![1.0, 1.0, 0.0, 0.0]
        );
    }

    #[test]
    fn a_new_edge_restarts_the_gate() {
        let mut node = TrigToGate::new(3);
        assert_eq!(
            gates(&mut node, &[1.0, 0.0, 1.0, 0.0, 0.0, 0.0]),
            vec![1.0, 1.0, 1.0, 1.0, 1.0, 0.0]
        );
    }

    #[test]
    fn gate_time_is_at_least_one_sample() {
        let mut node = TrigToGate::new(0);
        assert_eq!(gates(&mut node, &[1.0, 0.0]), vec![1.0, 0.0]);
    }

    #[test]
    fn block_and_tick_paths_agree() {
        use crate::{config::Config, harness::build_placeholder_context};

        let trigs: Vec<f32> = (0..256)
            .map(|i| if i % 17 == 0 { 1.0 } else { 0.0 })
            .collect();

        let mut node = TrigToGate::new(5);
        let mut ctx = build_placeholder_context(Config {
            block_size: 256,
            channels: 1,
            rt_capacity: 0,
            sample_rate: 48_000,
        });

        let mut block = [0.0f32; 256];
        node.process(&mut ctx, &[Some(&trigs)], &mut [&mut block]);

        let mut tick_node = TrigToGate::new(5);
        assert_eq!(gates(&mut tick_node, &trigs), block.to_vec());
    }

    #[test]
    fn unpatched_input_writes_nothing() {
        use crate::{config::Config, harness::build_placeholder_context};

        let mut node = TrigToGate::new(4);
        let mut ctx = build_placeholder_context(Config {
            block_size: 4,
            channels: 1,
            rt_capacity: 0,
            sample_rate: 48_000,
        });

        let mut gate = [-1.0f32; 4];
        node.process(&mut ctx, &[None], &mut [&mut gate]);

        assert_eq!(gate, [-1.0; 4]);
    }
}
