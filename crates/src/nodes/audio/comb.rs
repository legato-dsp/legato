use crate::{
    context::AudioContext,
    msg::{NodeMessage, RtValue},
    node::{Inputs, Node},
    ports::{PortBuilder, Ports},
    resources::{delay::ResourceDelay, window::Window},
};

/// A feedback comb filter: `y[n] = x[n] + feedback * y[n - M]`.
///
/// The delayed tap is read with cubic interpolation, so `delay_length` can be
/// a fractional number of samples and modulated smoothly at audio rate. This is
/// the classic building block for the parallel section of a Schroeder/Moorer
/// reverb (several of these in parallel, each with a slightly different,
/// mutually-prime delay length). For a damped (Freeverb/Moorer) comb use
/// [`CombLp`].
///
/// All channels share one flat, channel-major backing allocation (see
/// [`crate::nodes::audio::tap::DelayTap`] for the rationale); each channel keeps
/// a lightweight [`ResourceDelay`] cursor over its slice.
#[derive(Clone)]
pub struct Comb {
    feedback: f32,
    delay_length_samples: f32,
    data: Box<[f32]>,
    delays: Vec<ResourceDelay>,
    cap: usize,
    chans: usize,
    ports: Ports,
}

impl Comb {
    pub fn new(chans: usize, feedback: f32, delay_length_samples: f32, cap: usize) -> Self {
        let cap = cap.next_power_of_two();
        let data = vec![0.0; chans * cap].into_boxed_slice();
        let delays = (0..chans)
            .map(|c| {
                ResourceDelay::new(Window {
                    start: c * cap,
                    len: cap,
                })
            })
            .collect();

        Self {
            feedback: feedback.clamp(0.0, 0.98),
            delay_length_samples,
            data,
            delays,
            cap,
            chans,
            ports: PortBuilder::default()
                .audio_in(chans)
                .audio_in_named(&["delay_length", "feedback"])
                .audio_out(chans)
                .build(),
        }
    }

    fn process_modulated(
        &mut self,
        ctx: &mut AudioContext,
        inputs: &Inputs,
        outputs: &mut [&mut [f32]],
    ) {
        let sr = ctx.get_config().sample_rate as f32;
        let delay_length_port = inputs.get(self.chans).and_then(|x| *x);
        let feedback_port = inputs.get(self.chans + 1).and_then(|x| *x);
        let max_capacity = self.cap as f32;

        for c in 0..self.chans {
            let input = inputs[c].unwrap();
            let base = c * self.cap;
            let chan_data = &mut self.data[base..base + self.cap];
            let delay = &mut self.delays[c];
            let output = &mut outputs[c];

            for i in 0..input.len() {
                let delay_length_samples = delay_length_port
                    .map_or(self.delay_length_samples, |buf| sr * (buf[i] / 1000.0))
                    .clamp(1.0, max_capacity);
                let feedback = feedback_port
                    .map_or(self.feedback, |buf| buf[i])
                    .clamp(0.0, 0.98);

                let delayed = delay.get_delay_cubic(chan_data, delay_length_samples);
                let write = input[i] + feedback * delayed;
                delay.push(chan_data, write);
                output[i] = write;
            }
        }
    }

    fn process_static(
        &mut self,
        _: &mut AudioContext,
        inputs: &Inputs,
        outputs: &mut [&mut [f32]],
    ) {
        let delay_length_samples = self.delay_length_samples.clamp(1.0, self.cap as f32);
        let feedback = self.feedback;

        for c in 0..self.chans {
            let input = inputs[c].unwrap();
            let base = c * self.cap;
            let chan_data = &mut self.data[base..base + self.cap];
            let delay = &mut self.delays[c];
            let output = &mut outputs[c];

            for i in 0..input.len() {
                let delayed = delay.get_delay_cubic(chan_data, delay_length_samples);
                let write = input[i] + feedback * delayed;
                delay.push(chan_data, write);
                output[i] = write;
            }
        }
    }
}

impl Node for Comb {
    fn process(&mut self, ctx: &mut AudioContext, inputs: &Inputs, outputs: &mut [&mut [f32]]) {
        // Branch once: take the modulated path only if something is actually
        // patched into a control port, so the common static case has no
        // per-sample `map_or`.
        let modulated = inputs.get(self.chans).and_then(|x| *x).is_some()
            || inputs.get(self.chans + 1).and_then(|x| *x).is_some();

        if modulated {
            self.process_modulated(ctx, inputs, outputs);
        } else {
            self.process_static(ctx, inputs, outputs);
        }
    }
    fn ports(&self) -> &Ports {
        &self.ports
    }
    fn handle_msg(&mut self, msg: NodeMessage) {
        if let NodeMessage::SetParam(inner) = msg {
            match (inner.param_name, inner.value) {
                ("feedback", RtValue::F32(val)) => self.feedback = val.clamp(0.0, 0.98),
                ("feedback", RtValue::U32(val)) => self.feedback = (val as f32).clamp(0.0, 0.98),
                ("delay_length", RtValue::F32(val)) => {
                    self.delay_length_samples = val.clamp(0.0, self.cap as f32)
                }
                ("delay_length", RtValue::U32(val)) => {
                    self.delay_length_samples = (val as f32).clamp(0.0, self.cap as f32)
                }
                _ => (),
            }
        }
    }
}

/// A feedback comb filter with a one-pole lowpass in the feedback path
/// (the Freeverb / Moorer "lowpass-feedback comb"):
///
/// `y[n] = x[n] + feedback * lp(y[n - M])`
///
/// `damp` is the one-pole coefficient in `[0, 1)`: `0` is a plain [`Comb`],
/// higher values roll the highs off faster on each pass, shortening the
/// high-frequency reverb time. Eight of these in parallel (their outputs summed)
/// followed by a few `allpass` sections is essentially a Freeverb. Output
/// convention matches [`Comb`] — the dry input passes through.
#[derive(Clone)]
pub struct CombLp {
    feedback: f32,
    damp: f32,
    delay_length_samples: f32,
    data: Box<[f32]>,
    delays: Vec<ResourceDelay>,
    /// One-pole lowpass state, per channel.
    lp_state: Vec<f32>,
    cap: usize,
    chans: usize,
    ports: Ports,
}

impl CombLp {
    pub fn new(
        chans: usize,
        feedback: f32,
        damp: f32,
        delay_length_samples: f32,
        cap: usize,
    ) -> Self {
        let cap = cap.next_power_of_two();
        let data = vec![0.0; chans * cap].into_boxed_slice();
        let delays = (0..chans)
            .map(|c| {
                ResourceDelay::new(Window {
                    start: c * cap,
                    len: cap,
                })
            })
            .collect();

        Self {
            feedback: feedback.clamp(0.0, 0.98),
            damp: damp.clamp(0.0, 0.999),
            delay_length_samples,
            data,
            delays,
            lp_state: vec![0.0; chans],
            cap,
            chans,
            ports: PortBuilder::default()
                .audio_in(chans)
                .audio_in_named(&["delay_length", "feedback", "damp"])
                .audio_out(chans)
                .build(),
        }
    }

    fn process_modulated(
        &mut self,
        ctx: &mut AudioContext,
        inputs: &Inputs,
        outputs: &mut [&mut [f32]],
    ) {
        let sr = ctx.get_config().sample_rate as f32;
        let delay_length_port = inputs.get(self.chans).and_then(|x| *x);
        let feedback_port = inputs.get(self.chans + 1).and_then(|x| *x);
        let damp_port = inputs.get(self.chans + 2).and_then(|x| *x);
        let max_capacity = self.cap as f32;

        for c in 0..self.chans {
            let input = inputs[c].unwrap();
            let base = c * self.cap;
            let chan_data = &mut self.data[base..base + self.cap];
            let delay = &mut self.delays[c];
            let output = &mut outputs[c];
            let mut lp = self.lp_state[c];

            for i in 0..input.len() {
                let delay_length_samples = delay_length_port
                    .map_or(self.delay_length_samples, |buf| sr * (buf[i] / 1000.0))
                    .clamp(1.0, max_capacity);
                let feedback = feedback_port
                    .map_or(self.feedback, |buf| buf[i])
                    .clamp(0.0, 0.98);
                let damp = damp_port.map_or(self.damp, |buf| buf[i]).clamp(0.0, 0.999);

                let delayed = delay.get_delay_cubic(chan_data, delay_length_samples);
                lp = (1.0 - damp) * delayed + damp * lp;
                let write = input[i] + feedback * lp;
                delay.push(chan_data, write);
                output[i] = write;
            }

            self.lp_state[c] = lp;
        }
    }

    fn process_static(
        &mut self,
        _: &mut AudioContext,
        inputs: &Inputs,
        outputs: &mut [&mut [f32]],
    ) {
        let delay_length_samples = self.delay_length_samples.clamp(1.0, self.cap as f32);
        let feedback = self.feedback;
        let damp = self.damp;
        let one_minus_damp = 1.0 - damp;

        for c in 0..self.chans {
            let input = inputs[c].unwrap();
            let base = c * self.cap;
            let chan_data = &mut self.data[base..base + self.cap];
            let delay = &mut self.delays[c];
            let output = &mut outputs[c];
            let mut lp = self.lp_state[c];

            for i in 0..input.len() {
                let delayed = delay.get_delay_cubic(chan_data, delay_length_samples);
                lp = one_minus_damp * delayed + damp * lp;
                let write = input[i] + feedback * lp;
                delay.push(chan_data, write);
                output[i] = write;
            }

            self.lp_state[c] = lp;
        }
    }
}

impl Node for CombLp {
    fn process(&mut self, ctx: &mut AudioContext, inputs: &Inputs, outputs: &mut [&mut [f32]]) {
        // Single branch at function entry: any patched control port takes the
        // modulated path, otherwise the static path avoids per-sample `map_or`.
        let modulated = inputs.get(self.chans).and_then(|x| *x).is_some()
            || inputs.get(self.chans + 1).and_then(|x| *x).is_some()
            || inputs.get(self.chans + 2).and_then(|x| *x).is_some();

        if modulated {
            self.process_modulated(ctx, inputs, outputs);
        } else {
            self.process_static(ctx, inputs, outputs);
        }
    }
    fn ports(&self) -> &Ports {
        &self.ports
    }
    fn handle_msg(&mut self, msg: NodeMessage) {
        if let NodeMessage::SetParam(inner) = msg {
            match (inner.param_name, inner.value) {
                ("feedback", RtValue::F32(val)) => self.feedback = val.clamp(0.0, 0.98),
                ("feedback", RtValue::U32(val)) => self.feedback = (val as f32).clamp(0.0, 0.98),
                ("damp", RtValue::F32(val)) => self.damp = val.clamp(0.0, 0.999),
                ("delay_length", RtValue::F32(val)) => {
                    self.delay_length_samples = val.clamp(0.0, self.cap as f32)
                }
                ("delay_length", RtValue::U32(val)) => {
                    self.delay_length_samples = (val as f32).clamp(0.0, self.cap as f32)
                }
                _ => (),
            }
        }
    }
}

use crate::{
    builder::{ResourceBuilderView, ValidationError},
    dsl::ir::DSLParams,
    node::DynNode,
    spec::NodeDefinition,
};

impl NodeDefinition for Comb {
    const NAME: &'static str = "comb";
    const DESCRIPTION: &'static str =
        "Feedback comb filter with configurable delay length and feedback";
    const REQUIRED_PARAMS: &'static [&'static str] = &["delay_length", "feedback", "chans"];
    const OPTIONAL_PARAMS: &'static [&'static str] = &["capacity"];

    fn create(
        rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        use std::time::Duration;
        let config = rb.get_config();
        let sr = config.sample_rate;
        let chans = p.get_usize("chans").unwrap_or(2);
        let delay_length = p
            .get_duration_ms("delay_length")
            .unwrap_or(Duration::from_millis(50));
        let delay_length_samples = sr as f32 * delay_length.as_secs_f32();
        let feedback = p.get_f32("feedback").unwrap_or(0.5);
        let mut capacity = p.get_usize("capacity").unwrap_or(sr);
        if capacity < (delay_length_samples as usize) {
            capacity = (delay_length_samples as usize) * 2;
        }
        Ok(Box::new(Self::new(
            chans,
            feedback,
            delay_length_samples,
            capacity,
        )))
    }
}

impl NodeDefinition for CombLp {
    const NAME: &'static str = "comb_lp";
    const DESCRIPTION: &'static str =
        "Feedback comb filter with a one-pole lowpass (damping) in the feedback path";
    const REQUIRED_PARAMS: &'static [&'static str] = &["delay_length", "feedback", "chans"];
    const OPTIONAL_PARAMS: &'static [&'static str] = &["damp", "capacity"];

    fn create(
        rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        use std::time::Duration;
        let config = rb.get_config();
        let sr = config.sample_rate;
        let chans = p.get_usize("chans").unwrap_or(2);
        let delay_length = p
            .get_duration_ms("delay_length")
            .unwrap_or(Duration::from_millis(50));
        let delay_length_samples = sr as f32 * delay_length.as_secs_f32();
        let feedback = p.get_f32("feedback").unwrap_or(0.5);
        let damp = p.get_f32("damp").unwrap_or(0.2);
        let mut capacity = p.get_usize("capacity").unwrap_or(sr);
        if capacity < (delay_length_samples as usize) {
            capacity = (delay_length_samples as usize) * 2;
        }
        Ok(Box::new(Self::new(
            chans,
            feedback,
            damp,
            delay_length_samples,
            capacity,
        )))
    }
}
