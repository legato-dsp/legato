use crate::mini_graph::node::Node;
use crate::mini_graph::buffer::Frame;
use crate::mini_graph::bang::Bang;

// "Adopted" from FunDSP

#[derive(Default, Clone, Copy)]
struct MoogFilterState {
    s0: f32,
    s1: f32,
    s2: f32,
    s3: f32,
    px: f32,
    ps0: f32,
    ps1: f32,
    ps2: f32
}

pub struct MoogFilter<const BUFFER_SIZE: usize, const CHANNEL_COUNT: usize> {
    sample_rate: f32,
    cutoff: f32,
    p: f32,
    q: f32,
    k: f32,
    rez: f32,
    filters: [MoogFilterState; CHANNEL_COUNT]
}
impl<const BUFFER_SIZE: usize, const CHANNEL_COUNT: usize> MoogFilter<BUFFER_SIZE, CHANNEL_COUNT> {
    pub fn new(sample_rate: u32) -> Self {
        let mut new_filter = Self {
            sample_rate: sample_rate as f32,
            cutoff: 0.0,
            p: 0.0,
            q: 0.0,
            k: 0.0,
            rez: 0.0,
            filters: [MoogFilterState::default(); CHANNEL_COUNT]
        };

        const DEFAULT_CUTOFF: f32 = 2400.0;
        const DEFAULT_Q: f32 = 0.500;

        new_filter.set_cutoff_q(DEFAULT_CUTOFF, DEFAULT_Q);

        new_filter
    }
    #[inline(always)]
    pub fn set_cutoff_q(&mut self, cutoff: f32, q: f32){
        // Tunings taken from FunDSP
        self.cutoff = cutoff;
        self.q = q;
        let c = 2.0 * cutoff / self.sample_rate;
        self.p = c * (1.8 - 0.8 * c);
        self.k = 2.0 * (c * std::f32::consts::PI * 0.5).sin() - 1.0;
        let t1 = (1.0 - self.p) * 1.386249;
        let t2 = 12.0 + t1 * t1;
        self.rez = q * (t2 + 6.0 * t1) / (t2 - 6.0 * t1);
    }
}

impl<const BUFFER_SIZE: usize, const CHANNEL_COUNT: usize> Node<BUFFER_SIZE, CHANNEL_COUNT> for MoogFilter<BUFFER_SIZE, CHANNEL_COUNT> {
    fn process(&mut self, inputs: &[Frame<BUFFER_SIZE, CHANNEL_COUNT>], output: &mut Frame<BUFFER_SIZE, CHANNEL_COUNT>) {
        let input = match inputs.get(0) {
            Some(input) => input,
            None => return,
        };
        
        let cutoff = inputs.get(1);

        for n in 0..BUFFER_SIZE {
            if let Some(cutoff_frame) = cutoff {
                let new_cutoff = cutoff_frame[0][n];
                self.set_cutoff_q(new_cutoff, self.q); // use current Q
            }

            for c in 0..CHANNEL_COUNT {
                let filter = &mut self.filters[c];
                let x = -self.rez * filter.s3 + input[c][n];

                filter.s0 = (x + filter.px) * self.p - self.k * filter.s0;
                filter.s1 = (filter.s0 + filter.ps0) * self.p - self.k * filter.s1;
                filter.s2 = (filter.s1 + filter.ps1) * self.p - self.k * filter.s2;
                filter.s3 = ((filter.s2 + filter.ps2) * self.p - self.k * filter.s3).tanh();

                filter.px = x;
                filter.ps0 = filter.s0;
                filter.ps1 = filter.s1;
                filter.ps2 = filter.s2;

                output[c][n] = filter.s3;
            }
        }
    }
}   