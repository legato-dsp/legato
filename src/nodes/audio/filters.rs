use std::f32::consts::PI;

use crate::mini_graph::{buffer::Frame, node::Node};


#[derive(Copy, Clone)]
pub enum FilterType {
    LowPass,
    BandPass,
    HighPass,
    Notch,
    Peak,
    AllPass,
    Bell,
    LowShelf,
    HighShelf,
}
#[derive(Copy, Clone, Default)]
struct SvfState {
    ic1eq: f32,
    ic2eq: f32
}


#[derive(Copy, Clone, Default)]
struct SvfCoefficients {
    a1: f32,
    a2: f32,
    a3: f32,
    m0: f32,
    m1: f32,
    m2: f32,
}

pub struct Svf <const C: usize> {
    filter_type: FilterType,
    sample_rate: f32,
    cutoff: f32,
    gain: f32,
    q: f32,
    // Filter state for each channel
    filter_state: [SvfState; C],
    // Filter Coefficeints
    coefficients: SvfCoefficients
}

impl<const C: usize> Svf<C> {
    pub fn new(sample_rate: f32, filter_type: FilterType, cutoff: f32, gain: f32, q: f32) -> Self {
        let mut new_filter = 
        Self {
            sample_rate, 
            filter_type,
            cutoff,
            gain,
            q,
            filter_state: [SvfState::default();C],
            coefficients: SvfCoefficients::default()
        };

        new_filter.set(filter_type, sample_rate, cutoff, q, gain);

        new_filter
    }
    #[inline(always)]
    pub fn set(&mut self, filter_type: FilterType, sample_rate: f32, cutoff: f32, q: f32, gain: f32){
        self.filter_type = filter_type;
        self.sample_rate = sample_rate;
        self.cutoff = cutoff;
        self.q = q;
        self.gain = gain;

        match filter_type {
            FilterType::LowPass => {
                let g = (PI * self.cutoff / self.sample_rate).tan();
                let k = 1.0 / self.q;

                self.coefficients.a1 = 1.0 / (1.0 + g * (g + k));
                self.coefficients.a2 = g * self.coefficients.a1;
                self.coefficients.a3 = g * self.coefficients.a2;
                self.coefficients.m0 = 0.0;
                self.coefficients.m1 = 0.0;
                self.coefficients.m2 = 1.0;
            },
            FilterType::BandPass => {
                let g = (PI * self.cutoff / self.sample_rate).tan();
                let k = 1.0 / self.q;

                self.coefficients.a1 = 1.0 / (1.0 + g*(g + k));
                self.coefficients.a2 = g * self.coefficients.a1;
                self.coefficients.a3 = g*self.coefficients.a2;
                self.coefficients.m0 = 0.0;
                self.coefficients.m1 = 1.0;
                self.coefficients.m2 = 0.0;
            },
            FilterType::HighPass => {
                let g = (PI * self.cutoff / self.sample_rate).tan();
                let k = 1.0 / self.q;
                self.coefficients.a1 = 1.0 / (1.0 + g * (g + k));
                self.coefficients.a2 = g * self.coefficients.a1;
                self.coefficients.a3 = g * self.coefficients.a2;
                self.coefficients.m0 = 1.0;
                self.coefficients.m1 = -k;
                self.coefficients.m2 = -1.0;
            }
            FilterType::BandPass => {
                let g = (PI * self.cutoff / self.sample_rate).tan();
                let k = 1.0 / self.q;
                self.coefficients.a1 = 1.0 / (1.0 + g * (g + k));
                self.coefficients.a2 = g * self.coefficients.a1;
                self.coefficients.a3 = g * self.coefficients.a2;
                self.coefficients.m0 = 0.0;
                self.coefficients.m1 = 1.0;
                self.coefficients.m2 = 0.0;
            }
            FilterType::Notch => {
                let g = (PI * self.cutoff / self.sample_rate).tan();
                let k = 1.0 / self.q;
                self.coefficients.a1 = 1.0 / (1.0 + g * (g + k));
                self.coefficients.a2 = g * self.coefficients.a1;
                self.coefficients.a3 = g * self.coefficients.a2;
                self.coefficients.m0 = 1.0;
                self.coefficients.m1 = -k;
                self.coefficients.m2 = 0.0;
            }
            FilterType::Peak => {
                let g = (PI * self.cutoff / self.sample_rate).tan();

                let k = 1.0 / self.q;
                self.coefficients.a1 = 1.0 / (1.0 + g * (g + k));
                self.coefficients.a2 = g * self.coefficients.a1;
                self.coefficients.a3 = g * self.coefficients.a2;
                self.coefficients.m0 = 1.0;
                self.coefficients.m1 = -k;
                self.coefficients.m2 = -2.0;
            }
            FilterType::AllPass => {
                let g = (PI * self.cutoff / self.sample_rate).tan();
                let k = 1.0 / self.q;
                self.coefficients.a1 = 1.0 / (1.0 + g * (g + k));
                self.coefficients.a2 = g * self.coefficients.a1;
                self.coefficients.a3 = g * self.coefficients.a2;
                self.coefficients.m0 = 1.0;
                self.coefficients.m1 = -2.0 * k;
                self.coefficients.m2 = 0.0;
            }
            FilterType::Bell => {
                let a = f32::powf(
                    10.0,
                    self.gain / 40.0,
                );
                let g = (PI * self.cutoff / self.sample_rate).tan();

                let k = 1.0 / (self.q * a);
                self.coefficients.a1 = 1.0 / (1.0 + g * (g + k));
                self.coefficients.a2 = g * self.coefficients.a1;
                self.coefficients.a3 = g * self.coefficients.a2;
                self.coefficients.m0 = 1.0;
                self.coefficients.m1 = k * (a * a - 1.0);
                self.coefficients.m2 = 0.0;
            }
            FilterType::LowShelf => {
                let a = f32::powf(
                    10.0,
                    self.gain / 40.0,
                );
                let g = (PI * self.cutoff / self.sample_rate).tan() / f32::sqrt(a);
                let k = 1.0 / self.q;
                self.coefficients.a1 = 1.0 / (1.0 + g * (g + k));
                self.coefficients.a2 = g * self.coefficients.a1;
                self.coefficients.a3 = g * self.coefficients.a2;
                self.coefficients.m0 = 1.0;
                self.coefficients.m1 = k * (a - 1.0);
                self.coefficients.m2 = a * a - 1.0;
            }
            FilterType::HighShelf => {
                let a = f32::powf(
                    10.0,
                    self.gain / 40.0,
                );
                let g = (PI * self.cutoff / self.sample_rate).tan() * f32::sqrt(a);

                let k = 1.0 / self.q;
                self.coefficients.a1 = 1.0 / (1.0 + g * (g + k));
                self.coefficients.a2 = g * self.coefficients.a1;
                self.coefficients.a3 = g * self.coefficients.a2;
                self.coefficients.m0 = a * a;
                self.coefficients.m1 = k * (1.0 - a) * a;
                self.coefficients.m2 = 1.0 - a * a;
            }
            _ => ()
        }
    }
}

const CUTOFF_EPSILON: f32 = 1e-3;

impl<const N: usize, const C: usize> Node<N, C> for Svf<C>{
    fn process(&mut self, inputs: &[Frame<N, C>], output: &mut Frame<N, C>) {
        let input = match inputs.get(0) {
            Some(input) => input,
            None => return,
        };
        
        let cutoff = inputs.get(1);

        for n in 0..N {
            if let Some(cutoff_frame) = cutoff {
                let new_cutoff = cutoff_frame[0][n];
                if (new_cutoff - self.cutoff).abs() > CUTOFF_EPSILON {
                    self.set(self.filter_type, self.sample_rate, new_cutoff, self.q, self.gain);
                }
            }

            for c in 0..C {
                let filter_state = &mut self.filter_state[c];

                let v0 = input[c][n];

                let v3 = v0 - filter_state.ic2eq;

                let v1 = self.coefficients.a1 * filter_state.ic1eq + self.coefficients.a2 * v3;

                let v2 = filter_state.ic2eq + self.coefficients.a2 * filter_state.ic1eq + self.coefficients.a3 * v3;
                
                filter_state.ic1eq = 2.0 * v1 - filter_state.ic1eq;
                filter_state.ic2eq = 2.0 * v2 - filter_state.ic2eq;

                output[c][n] = self.coefficients.m0 * v0 + self.coefficients.m1 * v1 + self.coefficients.m2 * v2;
            }
        }
    }
}