use crate::mini_graph::node::Node;
use crate::mini_graph::bang::Bang;

pub struct Clock<const N: usize, const C: usize> {
    sample_rate: u32,
    is_ticking: bool,
    tick_period_samples: u64,
    samples_accum: u64,
}

impl<const N: usize, const C: usize> Clock<N, C> {
    pub fn new(sample_rate: u32, clock_rate: std::time::Duration) -> Self {
        let period_secs = clock_rate.as_secs_f32();
        let tick_period_samples = (period_secs * sample_rate as f32).round() as u64;

        Self {
            sample_rate,
            is_ticking: true,
            tick_period_samples,
            samples_accum: tick_period_samples, // so this fires immediately
        }
    }
}

impl<const N: usize, const C: usize> Node<N, C> for Clock<N, C> {
    fn handle_bang(&mut self, inputs: &[Bang], output: &mut Bang) {
        if let Some(b) = inputs.get(0) {
            match b {
                Bang::Bang => self.is_ticking = !self.is_ticking,
                Bang::BangBool(val) => self.is_ticking = *val,
                _ => (),
            }
        }
        if let Some(b) = inputs.get(1) {
            match b {
                Bang::BangF32(val) => {
                    let new_period = (*val * self.sample_rate as f32).round() as u64;
                    self.tick_period_samples = new_period;
                }
                Bang::BangU32(val) => {
                    let new_period = (*val as f32 * self.sample_rate as f32).round() as u64;
                    self.tick_period_samples = new_period;
                }
                _ => (),
            }
        }

        if !self.is_ticking {
            *output = Bang::Empty;
            return;
        }

        self.samples_accum += N as u64;
        
        if self.samples_accum >= self.tick_period_samples {
            *output = Bang::Bang;
            self.samples_accum -= self.tick_period_samples;
        } else {
            *output = Bang::Empty;
        }
    }
}
