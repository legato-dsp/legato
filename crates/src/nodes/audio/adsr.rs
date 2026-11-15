use crate::{
    math::lerp,
    msg::{NodeMessage, RtValue},
    node::Node,
    ports::{PortBuilder, Ports},
};

// TODO: I think I may be able to rewrite this branchless, not sure if hot enough path to warrant?

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum AdsrState {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Clone)]
pub struct Adsr {
    chans: usize,
    attack_ms: f32,
    decay_ms: f32,
    sustain_amount: f32,
    release_ms: f32,
    state: AdsrState,
    state_delta_t: f32,
    attack_starting_level: f32,
    release_starting_level: f32,
    gate_open: bool,
    trig_high: bool,
    ports: Ports,
}

impl Adsr {
    pub fn new(chans: usize, attack: f32, decay: f32, sustain: f32, release: f32) -> Self {
        Self {
            chans,
            attack_ms: attack,
            decay_ms: decay,
            sustain_amount: sustain,
            release_ms: release,
            state: AdsrState::Idle,
            state_delta_t: 0.0,
            attack_starting_level: 0.0,
            release_starting_level: 0.0,
            gate_open: false,
            trig_high: false,
            ports: PortBuilder::default()
                .control_in_named(&["gate"])
                .required_in()
                .audio_in(chans)
                .default_in()
                .silent_in()
                .control_in_named(&["trig"])
                .audio_out(chans)
                .build(),
        }
    }

    #[inline(always)]
    fn on_gate(&mut self) {
        self.attack_starting_level = self.get_gain();
        self.state = AdsrState::Attack;
        self.state_delta_t = 0.0;
    }

    #[inline(always)]
    fn on_gate_release(&mut self) {
        // Get the starting gain for the release filter
        self.release_starting_level = self.get_gain();
        self.state = AdsrState::Release;
        self.state_delta_t = 0.0;
    }

    #[inline(always)]
    fn get_gain(&self) -> f32 {
        // TODO: Exponential? Maybe a LUT implementation as well.
        match self.state {
            AdsrState::Idle => 0.0,
            AdsrState::Attack => {
                let t = (self.state_delta_t / self.attack_ms).min(1.0);
                lerp(self.attack_starting_level, 1.0, t)
            }
            AdsrState::Decay => {
                let t = (self.state_delta_t / self.decay_ms).min(1.0);
                lerp(1.0, self.sustain_amount, t)
            }
            AdsrState::Sustain => self.sustain_amount,
            AdsrState::Release => {
                let t = (self.state_delta_t / self.release_ms).min(1.0);
                lerp(self.release_starting_level, 0.0, t)
            }
        }
    }

    #[inline(always)]
    fn update_gain(&mut self) {
        match self.state {
            AdsrState::Attack if self.state_delta_t >= self.attack_ms => {
                self.state = AdsrState::Decay;
                self.state_delta_t = 0.0;
            }
            AdsrState::Decay if self.state_delta_t >= self.decay_ms => {
                self.state = AdsrState::Sustain;
                self.state_delta_t = 0.0;
            }
            AdsrState::Release if self.state_delta_t >= self.release_ms => {
                self.state = AdsrState::Idle;
                self.state_delta_t = 0.0;
            }
            _ => (),
        }
    }
}

impl Node for Adsr {
    fn process(
        &mut self,
        ctx: &mut crate::context::AudioContext,
        inputs: &crate::node::Inputs,
        outputs: &mut [&mut [f32]],
    ) {
        let config = ctx.get_config();
        let sr = config.sample_rate;
        let block_size = config.block_size;

        let dt = 1000.0 / sr as f32; // Using ms here

        let gate_chan = inputs[0].expect("ADSR Filter requires gate channel at index 0!");
        let trig_chan = inputs[self.chans + 1];

        // TODO: A lot of branches here. May be worth writing branchless or with a simple LUT
        for n in 0..block_size {
            // Only act on gate edges, so a held gate runs the full attack/decay/sustain.
            let gate_open = gate_chan[n] > 0.5;
            let trig_high = trig_chan.is_some_and(|t| t[n] > 0.5);
            let retrig = trig_high && !self.trig_high && gate_open && self.gate_open;
            if (gate_open && !self.gate_open) || retrig {
                self.on_gate();
            } else if !gate_open && self.gate_open {
                self.on_gate_release();
            }
            self.gate_open = gate_open;
            self.trig_high = trig_high;

            let gain = self.get_gain();

            for c in 0..self.chans {
                outputs[c][n] = inputs[c + 1].expect("ADSR has no optional channels.")[n] * gain;
            }

            self.state_delta_t += dt;

            self.update_gain();
        }
    }
    fn handle_msg(&mut self, msg: crate::msg::NodeMessage) {
        // Leaving this issue here for the time being so that I remember other states
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

    fn runtime_params(&self) -> Option<Vec<&str>> {
        Some(vec!["attack", "decay", "sustain", "release"])
    }
    fn ports(&self) -> &Ports {
        &self.ports
    }
}

use crate::{
    builder::{ResourceBuilderView, ValidationError},
    dsl::ir::DSLParams,
    node::DynNode,
    spec::NodeDefinition,
};

impl NodeDefinition for Adsr {
    const NAME: &'static str = "adsr";
    const DESCRIPTION: &'static str = "Attack-decay-sustain-release envelope generator";
    const REQUIRED_PARAMS: &'static [&'static str] =
        &["attack", "decay", "sustain", "release", "chans"];
    const OPTIONAL_PARAMS: &'static [&'static str] = &[];

    fn create(
        _rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        let attack = p.require_f32("attack")?;
        let decay = p.require_f32("decay")?;
        let sustain = p.require_f32("sustain")?;
        let release = p.require_f32("release")?;
        let chans = p.require_usize("chans")?;
        Ok(Box::new(Self::new(chans, attack, decay, sustain, release)))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{config::Config, harness::build_placeholder_context};

    fn run(node: &mut Adsr, gates: &[f32]) -> Vec<f32> {
        run_with_trig(node, gates, None)
    }

    fn run_with_trig(node: &mut Adsr, gates: &[f32], trigs: Option<&[f32]>) -> Vec<f32> {
        let mut ctx = build_placeholder_context(Config {
            block_size: gates.len(),
            channels: 1,
            rt_capacity: 0,
            sample_rate: 1_000,
        });
        let ones = vec![1.0f32; gates.len()];
        let mut out = vec![0.0f32; gates.len()];
        node.process(&mut ctx, &[Some(gates), Some(&ones), trigs], &mut [&mut out]);
        out
    }

    #[test]
    fn trig_while_gate_held_restarts_attack() {
        let mut node = Adsr::new(1, 2.0, 2.0, 0.3, 10.0);
        let mut trigs = [0.0; 16];
        trigs[10] = 1.0;
        let out = run_with_trig(&mut node, &[1.0; 16], Some(&trigs));
        assert!((out[9] - 0.3).abs() < 1e-6);
        assert!(out[11] > out[10] && out[11] > 0.3);
    }

    #[test]
    fn trig_without_gate_is_ignored() {
        let mut node = Adsr::new(1, 2.0, 2.0, 0.3, 10.0);
        let mut trigs = [0.0; 8];
        trigs[2] = 1.0;
        let out = run_with_trig(&mut node, &[0.0; 8], Some(&trigs));
        assert!(out.iter().all(|&x| x == 0.0));
    }

    #[test]
    fn trig_on_the_rising_gate_edge_triggers_once() {
        let mut with = Adsr::new(1, 4.0, 2.0, 0.3, 10.0);
        let mut without = Adsr::new(1, 4.0, 2.0, 0.3, 10.0);
        let mut trigs = [0.0; 8];
        trigs[0] = 1.0;
        let gates = [1.0; 8];
        assert_eq!(
            run_with_trig(&mut with, &gates, Some(&trigs)),
            run(&mut without, &gates)
        );
    }

    #[test]
    fn held_gate_settles_to_sustain() {
        let mut node = Adsr::new(1, 2.0, 2.0, 0.3, 10.0);
        let out = run(&mut node, &[1.0; 16]);
        assert!((out.last().unwrap() - 0.3).abs() < 1e-6);
    }

    #[test]
    fn velocity_scaled_gate_triggers() {
        let mut node = Adsr::new(1, 2.0, 2.0, 0.3, 10.0);
        let out = run(&mut node, &[0.7; 16]);
        assert!((out.last().unwrap() - 0.3).abs() < 1e-6);
    }
}
