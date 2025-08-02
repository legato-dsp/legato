use crate::mini_graph::bang::Bang;
use crate::mini_graph::node::Node;
use crate::mini_graph::buffer::Frame;

enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

pub struct ADSR<const N: usize, const C: usize> {
    attack_time:  f32,
    decay_time:   f32,
    sustain_time: f32,     
    release_time: f32,

    sustain_level: f32,    

    stage: Stage,
    time_in_stage:    f32,
    release_start_level: f32,

    sample_rate: f32,
}

impl<const N: usize, const C: usize> ADSR<N, C> {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            attack_time:   0.1,
            decay_time:    0.2,
            sustain_time:  0.0,
            release_time:  0.2,

            sustain_level: 0.1,

            stage: Stage::Idle,
            time_in_stage: 0.0,
            release_start_level: 0.0,

            sample_rate: sample_rate as f32,
        }
    }

    fn note_on(&mut self) {
        self.stage = Stage::Attack;
        self.time_in_stage = 0.0;
    }

    fn note_off(&mut self) {
        let current = self.current_level();
        self.release_start_level = current;
        self.stage = Stage::Release;
        self.time_in_stage = 0.0;
    }

    fn current_level(&self) -> f32 {
        match self.stage {
            Stage::Idle    => 0.0,
            Stage::Attack  => (self.time_in_stage / self.attack_time).min(1.0),
            Stage::Decay   => {
                let t = (self.time_in_stage / self.decay_time).min(1.0);
                1.0 + t * (self.sustain_level - 1.0) // lerp(1.0, sustain_level, t)
            }
            Stage::Sustain => self.sustain_level,
            Stage::Release => {
                let t = (self.time_in_stage / self.release_time).min(1.0);
                self.release_start_level * (1.0 - t) // lerp(release_start, 0.0, t)
            }
        }
    }
}

impl<const N: usize, const C: usize> Node<N, C> for ADSR<N, C> {
    fn process(&mut self, inputs: &[Frame<N, C>], output: &mut Frame<N, C>) {
        let dt = 1.0 / self.sample_rate;
        let input = inputs[0];

        for n in 0..N {
            let level = self.current_level();

            for c in 0..C {
                output[c][n] = input[c][n] * level;
            }

            self.time_in_stage += dt;
            match self.stage {
                Stage::Attack if self.time_in_stage >= self.attack_time => {
                    self.stage = Stage::Decay;
                    self.time_in_stage = 0.0;
                }
                Stage::Decay if self.time_in_stage >= self.decay_time => {
                    if self.sustain_time > 0.0 {
                        self.stage = Stage::Sustain;
                        self.time_in_stage = 0.0;
                    } else {
                        self.stage = Stage::Sustain;
                    }
                }
                Stage::Sustain if self.sustain_time > 0.0
                    && self.time_in_stage >= self.sustain_time =>
                {
                    self.note_off();
                }
                Stage::Release if self.time_in_stage >= self.release_time => {
                    self.stage = Stage::Idle;
                    self.time_in_stage = 0.0;
                }
                _ => {}
            }
        }
    }

    fn handle_bang(&mut self, inputs: &[Bang], _: &mut Bang) {
        for &bang in inputs {
            if let Bang::Bang = bang {
                if matches!(self.stage, Stage::Idle) {
                    self.note_on();
                } else {
                    self.note_off();
                }
            }
        }
    }
}
