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
    ports: Ports,
}

impl Adsr {
    pub fn new(chans: usize, attack: f32, decay: f32, sustain: f32, release: f32) -> Self {
        Self {
            chans: chans,
            attack_ms: attack,
            decay_ms: decay,
            sustain_amount: sustain,
            release_ms: release,
            state: AdsrState::Idle,
            state_delta_t: 0.0,
            attack_starting_level: 0.0,
            release_starting_level: 0.0,
            ports: PortBuilder::default()
                .audio_in_named(&["gate"])
                .audio_in(chans)
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
        outputs: &mut crate::node::Outputs,
    ) {
        let config = ctx.get_config();
        let sr = config.sample_rate;
        let block_size = config.block_size;

        let dt = 1000.0 / sr as f32; // Using ms here

        let gate_chan = inputs[0].expect("ADSR Filter requires gate channel at index 0!");

        // TODO: A lot of branches here. May be worth writing branchless or with a simple LUT
        for n in 0..block_size {
            let gate_sample = gate_chan[n];
            // If we are released or idle, gate on
            if gate_sample == 1.0
                && (self.state == AdsrState::Idle || self.state == AdsrState::Release)
            {
                self.on_gate();
            }
            // If we are active and get a release
            if gate_sample == 0.0 && self.state != AdsrState::Idle {
                self.on_gate_release();
            }

            let gain = self.get_gain();

            for c in 0..self.chans {
                outputs[c][n] = inputs[c + 1].expect("ADSR has no optional channels.")[n] * gain;
            }

            self.state_delta_t += dt;

            self.update_gain();
        }
    }
    fn handle_msg(&mut self, msg: crate::msg::NodeMessage) {
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
