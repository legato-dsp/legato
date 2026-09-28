use crate::{
    builder::{ResourceBuilderView, ValidationError},
    context::AudioContext,
    dsl::ir::DSLParams,
    math::lerp,
    msg::{NodeMessage, RtValue},
    node::{DynNode, Inputs, Node},
    ports::{PortBuilder, Ports},
    spec::NodeDefinition,
};

#[derive(Clone, PartialEq, Eq, Debug)]
enum EnvState {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// Unlike ADSR, this outputs an envelope value, for more usecases
#[derive(Clone)]
pub struct Env {
    attack_ms: f32,
    decay_ms: f32,
    sustain_amount: f32,
    release_ms: f32,
    state: EnvState,
    state_delta_t: f32,
    attack_starting_level: f32,
    release_starting_level: f32,
    last_gate: f32,
    ports: Ports,
}

impl Env {
    pub fn new(attack: f32, decay: f32, sustain: f32, release: f32) -> Self {
        Self {
            attack_ms: attack,
            decay_ms: decay,
            sustain_amount: sustain,
            release_ms: release,
            state: EnvState::Idle,
            state_delta_t: 0.0,
            attack_starting_level: 0.0,
            release_starting_level: 0.0,
            last_gate: 0.0,
            ports: PortBuilder::default()
                .control_in_named(&["gate"])
                .default_in()
                .control_out_named(&["env"])
                .default_out()
                .build(),
        }
    }

    #[inline(always)]
    fn on_gate(&mut self) {
        self.attack_starting_level = self.get_gain();
        self.state = EnvState::Attack;
        self.state_delta_t = 0.0;
    }

    #[inline(always)]
    fn on_gate_release(&mut self) {
        self.release_starting_level = self.get_gain();
        self.state = EnvState::Release;
        self.state_delta_t = 0.0;
    }

    #[inline(always)]
    fn get_gain(&self) -> f32 {
        match self.state {
            EnvState::Idle => 0.0,
            EnvState::Attack => {
                let t = (self.state_delta_t / self.attack_ms).min(1.0);
                lerp(self.attack_starting_level, 1.0, t)
            }
            EnvState::Decay => {
                let t = (self.state_delta_t / self.decay_ms).min(1.0);
                lerp(1.0, self.sustain_amount, t)
            }
            EnvState::Sustain => self.sustain_amount,
            EnvState::Release => {
                let t = (self.state_delta_t / self.release_ms).min(1.0);
                lerp(self.release_starting_level, 0.0, t)
            }
        }
    }

    #[inline(always)]
    fn update_gain(&mut self) {
        match self.state {
            EnvState::Attack if self.state_delta_t >= self.attack_ms => {
                self.state = EnvState::Decay;
                self.state_delta_t = 0.0;
            }
            EnvState::Decay if self.state_delta_t >= self.decay_ms => {
                self.state = EnvState::Sustain;
                self.state_delta_t = 0.0;
            }
            EnvState::Release if self.state_delta_t >= self.release_ms => {
                self.state = EnvState::Idle;
                self.state_delta_t = 0.0;
            }
            _ => (),
        }
    }
}

impl Env {
    pub fn from_params(
        _rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Self, ValidationError> {
        let attack = p.get_f32("attack").unwrap_or(5.0);
        let decay = p.get_f32("decay").unwrap_or(200.0);
        let sustain = p.get_f32("sustain").unwrap_or(0.7);
        let release = p.get_f32("release").unwrap_or(300.0);
        Ok(Self::new(attack, decay, sustain, release))
    }
}

impl NodeDefinition for Env {
    const NAME: &'static str = "env";
    const DESCRIPTION: &'static str = "Edge-triggered ADSR envelope with a control-kind output, for use as a modulation source such as `env >> map >> svf.cutoff`.";
    const REQUIRED_PARAMS: &'static [&'static str] = &[];
    const OPTIONAL_PARAMS: &'static [&'static str] = &["attack", "decay", "sustain", "release"];

    fn create(
        rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        Ok(Box::new(Self::from_params(rb, p)?))
    }
}

impl Node for Env {
    fn process(&mut self, ctx: &mut AudioContext, inputs: &Inputs, outputs: &mut [&mut [f32]]) {
        let config = ctx.get_config();
        let dt = 1000.0 / config.sample_rate as f32;

        let Some(gate_chan) = inputs[0] else { return };

        for n in 0..config.block_size {
            let gate = gate_chan[n];
            if gate > 0.5 && self.last_gate <= 0.5 {
                self.on_gate();
            } else if gate <= 0.5 && self.last_gate > 0.5 && self.state != EnvState::Idle {
                self.on_gate_release();
            }
            self.last_gate = gate;

            outputs[0][n] = self.get_gain();

            self.state_delta_t += dt;
            self.update_gain();
        }
    }

    fn handle_msg(&mut self, msg: NodeMessage) {
        if let NodeMessage::SetParam(inner) = msg {
            match (inner.param_name, inner.value) {
                ("attack", RtValue::F32(x)) => self.attack_ms = x,
                ("decay", RtValue::F32(x)) => self.decay_ms = x,
                ("sustain", RtValue::F32(x)) => self.sustain_amount = x,
                ("release", RtValue::F32(x)) => self.release_ms = x,
                _ => (),
            }
        }
    }

    fn ports(&self) -> &Ports {
        &self.ports
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{config::Config, harness::build_placeholder_context};

    fn run(node: &mut Env, gates: &[f32]) -> Vec<f32> {
        let mut ctx = build_placeholder_context(Config {
            block_size: gates.len(),
            channels: 1,
            rt_capacity: 0,
            sample_rate: 1_000,
        });
        let mut out = vec![0.0f32; gates.len()];
        node.process(&mut ctx, &[Some(gates)], &mut [&mut out]);
        out
    }

    #[test]
    fn attack_ramps_from_zero_on_a_rising_edge() {
        // 4 ms attack at 1 kHz => 4 samples to reach 1.0.
        let mut node = Env::new(4.0, 10.0, 0.5, 10.0);
        let out = run(&mut node, &[1.0; 5]);
        assert_eq!(out[0], 0.0);
        assert!(out[1] > 0.0 && out[1] < out[2] && out[2] < out[3]);
        assert!((out[4] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn settles_to_sustain_while_held() {
        let mut node = Env::new(1.0, 2.0, 0.3, 10.0);
        let out = run(&mut node, &[1.0; 16]);
        assert!((out.last().unwrap() - 0.3).abs() < 1e-6);
    }

    #[test]
    fn releases_to_zero_after_a_falling_edge() {
        let mut node = Env::new(1.0, 1.0, 0.5, 3.0);
        let mut gates = vec![1.0; 4];
        gates.extend([0.0; 6]);
        let out = run(&mut node, &gates);
        assert!((out.last().unwrap() - 0.0).abs() < 1e-6);
    }
}
