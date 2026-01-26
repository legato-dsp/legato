use std::f32::consts::PI;

use crate::{
    context::AudioContext,
    node::{Inputs, Node, Outputs},
    ports::{PortBuilder, Ports},
};

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
    ic2eq: f32,
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

const CUTOFF_EPSILON: f32 = 1e-3;

#[derive(Clone)]
pub struct Svf {
    filter_type: FilterType,
    sample_rate: f32,
    cutoff: f32,
    gain: f32,
    q: f32,
    // Filter state for each channel
    filter_state: Vec<SvfState>,
    // Filter coeffs
    coefficients: SvfCoefficients,
    ports: Ports,
}

impl Svf {
    pub fn new(
        sample_rate: f32,
        filter_type: FilterType,
        cutoff: f32,
        gain: f32,
        q: f32,
        chans: usize,
    ) -> Self {
        let mut new_filter = Self {
            sample_rate,
            filter_type,
            cutoff,
            gain,
            q,
            filter_state: vec![SvfState::default(); chans],
            coefficients: SvfCoefficients::default(),
            ports: PortBuilder::default()
                .audio_in(chans)
                .audio_in_named(&["cutoff"])
                .audio_out(chans)
                .build(),
        };

        new_filter.set(filter_type, sample_rate, cutoff, q, gain);

        new_filter
    }
    #[inline(always)]
    pub fn set(
        &mut self,
        filter_type: FilterType,
        sample_rate: f32,
        cutoff: f32,
        q: f32,
        gain: f32,
    ) {
        self.filter_type = filter_type;
        self.sample_rate = sample_rate;
        let cutoff = cutoff.max(1.0).min(0.49 * self.sample_rate);

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
                let a = f32::powf(10.0, self.gain / 40.0);
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
                let a = f32::powf(10.0, self.gain / 40.0);
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
                let a = f32::powf(10.0, self.gain / 40.0);
                let g = (PI * self.cutoff / self.sample_rate).tan() * f32::sqrt(a);

                let k = 1.0 / self.q;
                self.coefficients.a1 = 1.0 / (1.0 + g * (g + k));
                self.coefficients.a2 = g * self.coefficients.a1;
                self.coefficients.a3 = g * self.coefficients.a2;
                self.coefficients.m0 = a * a;
                self.coefficients.m1 = k * (1.0 - a) * a;
                self.coefficients.m2 = 1.0 - a * a;
            }
        }
    }

    // TODO: SIMD, maybe hold the coefficients in chunks, replace certain operations with SIMD polynomial approximations?

    fn process_with_cutoff(
        &mut self,
        ctx: &mut AudioContext,
        inputs: &Inputs,
        outputs: &mut [&mut [f32]],
    ) {
        let chans = self.ports.audio_out.len();
        let block_size = ctx.get_config().block_size;
        let cutoff_idx = chans; // chans index + 1

        let cutoff = inputs[cutoff_idx].unwrap();

        for n in 0..block_size {
            let new_cutoff = cutoff[n];
            let clamped_cutoff = new_cutoff.clamp(1.0, 0.49 * self.sample_rate);

            if (new_cutoff - self.cutoff).abs() > CUTOFF_EPSILON {
                self.set(
                    self.filter_type,
                    self.sample_rate,
                    clamped_cutoff,
                    self.q,
                    self.gain,
                );
            } else {
                self.cutoff = clamped_cutoff;
            }
            for c in 0..chans {
                let sample = inputs[c].unwrap()[n];
                let filter_state = &mut self.filter_state[c];

                let v0 = sample;
                let v3 = v0 - filter_state.ic2eq;

                let v1 = self.coefficients.a1 * filter_state.ic1eq + self.coefficients.a2 * v3;

                let v2 = filter_state.ic2eq
                    + self.coefficients.a2 * filter_state.ic1eq
                    + self.coefficients.a3 * v3;

                filter_state.ic1eq = 2.0 * v1 - filter_state.ic1eq;
                filter_state.ic2eq = 2.0 * v2 - filter_state.ic2eq;

                outputs[c][n] = self.coefficients.m0 * v0
                    + self.coefficients.m1 * v1
                    + self.coefficients.m2 * v2;
            }
        }
    }

    fn process_without_cutoff(
        &mut self,
        _: &mut AudioContext,
        inputs: &Inputs,
        outputs: &mut [&mut [f32]],
    ) {
        for (c, (in_chan_out, out_chan)) in inputs.iter().zip(outputs.iter_mut()).enumerate() {
            if let Some(in_chan) = in_chan_out {
                for (n, sample) in in_chan.iter().enumerate() {
                    let filter_state = &mut self.filter_state[c];

                    let v0 = sample;
                    let v3 = v0 - filter_state.ic2eq;

                    let v1 = self.coefficients.a1 * filter_state.ic1eq + self.coefficients.a2 * v3;

                    let v2 = filter_state.ic2eq
                        + self.coefficients.a2 * filter_state.ic1eq
                        + self.coefficients.a3 * v3;

                    filter_state.ic1eq = 2.0 * v1 - filter_state.ic1eq;
                    filter_state.ic2eq = 2.0 * v2 - filter_state.ic2eq;

                    out_chan[n] = self.coefficients.m0 * v0
                        + self.coefficients.m1 * v1
                        + self.coefficients.m2 * v2;
                }
            }
        }
    }
}

impl Node for Svf {
    fn process(&mut self, ctx: &mut AudioContext, inputs: &Inputs, outputs: &mut [&mut [f32]]) {
        let cutoff_idx = self.ports.audio_in.len() - 1;
        if inputs[cutoff_idx].is_some() {
            self.process_with_cutoff(ctx, inputs, outputs);
        } else {
            self.process_without_cutoff(ctx, inputs, outputs);
        }
    }
    fn ports(&self) -> &Ports {
        &self.ports
    }
    fn handle_msg(&mut self, _msg: crate::msg::NodeMessage) {
        todo!()
    }
}
