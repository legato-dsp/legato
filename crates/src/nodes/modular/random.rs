use std::time::Duration;

use crate::{
    builder::{ResourceBuilderView, ValidationError},
    context::AudioContext,
    dsl::ir::DSLParams,
    node::{DynNode, Inputs, Node},
    persample::PerSampleNode,
    ports::{PortBuilder, Ports},
    rng::XorShift,
    spec::NodeDefinition,
};

#[derive(Clone)]
pub struct Random {
    rng: XorShift,
    /// The current stepped value being sustained.
    held: f32,
    /// The one-pole `smooth` output state.
    smoothed: f32,
    /// The one-pole coefficient for `smooth`, `0.0` = no smoothing.
    smoothing: f32,
    /// Probability in `[0, 1]` of taking a new value on each step.
    prob: f32,
    /// The output value range, `[lo, hi]`.
    range_lo: f32,
    range_hi: f32,
    /// The free-running clock period, in samples.
    period_in_samples: u32,
    /// Samples elapsed since the last internal-clock step.
    samples_since_step: u32,
    /// Previous `trig` input, for rising-edge detection.
    last_trig: f32,
    /// Used to convert `rate` modulation from ms to samples.
    sr: f32,
    ports: Ports,
}

impl Random {
    pub fn new(
        period_in_samples: u32,
        prob: f32,
        smoothing: f32,
        range: (f32, f32),
        sr: f32,
    ) -> Self {
        let mut rng = XorShift::seeded();
        let (range_lo, range_hi) = range;
        // Seed an initial value so `stepped` is meaningful before the first step,
        // which matters when an external clock has yet to send an edge.
        let held = range_lo + rng.unipolar() * (range_hi - range_lo);
        Self {
            rng,
            held,
            smoothed: held,
            smoothing,
            prob,
            range_lo,
            range_hi,
            period_in_samples: period_in_samples.max(1),
            samples_since_step: 0,
            last_trig: 0.0,
            sr,
            ports: PortBuilder::default()
                .control_in_named(&["trig"])
                .default_in()
                .control_in_named(&["rate", "prob"])
                .control_out_named(&["stepped"])
                .default_out()
                .control_out_named(&["smooth", "trig"])
                .build(),
        }
    }

    #[inline(always)]
    fn tick_inner(
        &mut self,
        trig_in: Option<f32>,
        rate: Option<f32>,
        prob: Option<f32>,
    ) -> (f32, f32, f32) {
        if let Some(rate_ms) = rate {
            self.period_in_samples = (((self.sr * (rate_ms / 1000.0)).floor()) as u32).max(1);
        }

        // A patched clock overrides the free-running one and steps on rising edges.
        let step_now = match trig_in {
            Some(trig) => {
                let rising_edge = trig > 0.5 && self.last_trig <= 0.5;
                self.last_trig = trig;
                rising_edge
            }
            None => {
                if self.samples_since_step >= self.period_in_samples {
                    self.samples_since_step = 0;
                }
                let fire = self.samples_since_step == 0;
                self.samples_since_step += 1;
                fire
            }
        };

        let prob = prob.unwrap_or(self.prob);

        let mut trig_out = 0.0;
        if step_now && self.rng.unipolar() < prob {
            self.held = self.range_lo + self.rng.unipolar() * (self.range_hi - self.range_lo);
            trig_out = 1.0;
        }

        self.smoothed = self.held * (1.0 - self.smoothing) + self.smoothed * self.smoothing;

        (self.held, self.smoothed, trig_out)
    }
}

impl Random {
    pub fn from_params(
        rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Self, ValidationError> {
        let sr = rb.config.sample_rate as f32;

        let rate = p
            .get_duration_ms("rate")
            .unwrap_or(Duration::from_millis(250));
        let period_in_samples = (rate.as_secs_f32() * sr).floor() as u32;

        let prob = p.get_f32("prob").unwrap_or(1.0);
        let smoothing = p.get_f32("smoothing").unwrap_or(0.0);

        let range = match p.get_array_f32("range") {
            Some(v) if v.len() == 2 => (v[0], v[1]),
            Some(_) => {
                return Err(ValidationError::InvalidParameter(
                    "random `range` takes exactly two numbers, e.g. range: [0, 12]".to_string(),
                ));
            }
            None => (0.0, 1.0),
        };

        Ok(Self::new(period_in_samples, prob, smoothing, range, sr))
    }
}

impl NodeDefinition for Random {
    const NAME: &'static str = "random";
    const DESCRIPTION: &'static str = "A modular random source. Each step it takes a new value in `range` with probability `prob`, emitting it on `stepped` (and a one-pole-`smooth`ed copy) while firing `trig`. Free-runs at `rate` ms, or steps on the rising edges of a patched `trig` input.";
    const REQUIRED_PARAMS: &'static [&'static str] = &[];
    const OPTIONAL_PARAMS: &'static [&'static str] = &["rate", "prob", "smoothing", "range"];

    fn create(
        rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        Ok(Box::new(Self::from_params(rb, p)?))
    }
}

impl Node for Random {
    fn process(&mut self, ctx: &mut AudioContext, inputs: &Inputs, outputs: &mut [&mut [f32]]) {
        let block_size = ctx.get_config().block_size;

        for i in 0..block_size {
            let trig = inputs[0].map(|b| b[i]);
            let rate = inputs[1].map(|b| b[i]);
            let prob = inputs[2].map(|b| b[i]);

            let (stepped, smooth, trig_out) = self.tick_inner(trig, rate, prob);

            outputs[0][i] = stepped;
            outputs[1][i] = smooth;
            outputs[2][i] = trig_out;
        }
    }

    fn ports(&self) -> &Ports {
        &self.ports
    }
}

impl PerSampleNode for Random {
    fn ports(&self) -> &Ports {
        &self.ports
    }

    fn tick(&mut self, in_frame: &[Option<f32>], out_frame: &mut [f32]) {
        let (stepped, smooth, trig) = self.tick_inner(in_frame[0], in_frame[1], in_frame[2]);

        out_frame[0] = stepped;
        out_frame[1] = smooth;
        out_frame[2] = trig;
    }
}
