use std::time::Duration;

use crate::{
    node::Node,
    nodes::{audio::grain::Grain, midi::voice::MIDDLE_C},
    ports::Ports,
    prelude::*,
    resources::DelayLineKey,
    rng::XorShift,
};

const NUM_GRAINS: usize = 2;

/// Amount of padding needed on either side for cubic interpolation,
/// we then have to use these offsets to safely spawn grains without
/// large discontinuities.
const CUBIC_NEWER: f32 = 1.0;
const CUBIC_OLDER: f32 = 2.0;

// TODO: DoD audit

pub struct GrainDelay {
    /// Keys corresponding to each channels delay line
    delay_keys: Vec<DelayLineKey>,
    /// Maximum capacity of the delay line, in samples
    delay_capacity: usize,
    /// Length of current delay line. Must be less than delay_capacity
    delay_len: usize,
    /// Without any damping, the feedback for grains folds and becomes very high pitched
    damping_a: f32,
    /// Current held state for interior lowpass filter
    damping_state: Vec<f32>,
    /// Grains stored per channel, nested collection is probably fine here
    grains: Vec<[Grain; NUM_GRAINS]>,
    /// Length of our grain
    grain_size: Duration,
    /// from 0.001 to 0.5, controls how rectangular (0) -> Hann shaped (0.5) the grains are
    shape: f32,
    /// The playhead rate shift relative to [`MIDDLE_C`], used to shift the grain sound any make shimmers
    freq: f32,
    /// Spawn position relative for each channel, relative to the delay line
    spawn_offset: Vec<f32>,
    /// Amount we move the spawn offset per sample, per channel
    scan: Vec<f32>,
    /// Amount of grain input that gets fed back into the delay line
    feedback: f32,
    /// 0..1, how much earlier than the down ramp a grain may spawn
    jitter: f32,
    /// Max random spawn offset either side of the scanner, in samples
    spray: f32,
    /// Per channel, scales the down-ramp spawn threshold, drawn from `[1 - jitter, 1]`
    spawn_threshold: Vec<f32>,
    // Note(Luke): If we don't add jitter, the delays start to flutter and make strange correlations.
    // I think a WSOLA algorithm might be the *correct* approach, but this is fine for now.
    rngs: Vec<XorShift>,
    ports: Ports,
}

impl GrainDelay {
    pub fn new(
        delay_keys: Vec<DelayLineKey>,
        delay_capacity: usize,
        delay_len: usize,
        damping: f32,
        chans: usize,
        shape: f32,
        scan: f32,
        grain_size: Duration,
        feedback: f32,
        jitter: f32,
        spray: f32,
    ) -> Self {
        debug_assert_eq!(delay_keys.len(), chans);

        let grains = vec![[Grain::default(); NUM_GRAINS]; chans];

        Self {
            delay_keys,
            delay_capacity,
            delay_len,
            damping_a: damping,
            damping_state: vec![0.0; chans],
            grains,
            grain_size,
            shape,
            spawn_offset: vec![delay_len as f32; chans],
            scan: vec![scan; chans],
            freq: MIDDLE_C,
            feedback,
            jitter: jitter.clamp(0.0, 1.0),
            spray: spray.max(0.0),
            spawn_threshold: vec![1.0; chans],
            rngs: (0..chans).map(|_| XorShift::seeded()).collect(),
            ports: PortBuilder::default()
                .audio_in(chans)
                .required_in()
                .default_in()
                .audio_out(chans)
                .default_out()
                .build(),
        }
    }
}

impl Node for GrainDelay {
    fn ports(&self) -> &Ports {
        &self.ports
    }
    fn process(&mut self, ctx: &mut AudioContext, inputs: &Inputs, outputs: &mut [&mut [f32]]) {
        let sr = ctx.get_config().sample_rate;
        let resources = ctx.get_resources_mut();

        let grain_len = self.grain_size.as_secs_f32() * sr as f32;
        let (lo, hi) = self.spawn_band(grain_len);
        let band_ok = hi > lo;

        // TODO(Luke): sample accurate modulation for various parameters
        for (c, (chan_in, chan_out)) in inputs.iter().zip(outputs.iter_mut()).enumerate() {
            let mut delay_line = resources.delay_line_view_mut(self.delay_keys[c]);
            let spawn_offset = &mut self.spawn_offset[c];

            let grains = &mut self.grains[c];
            let rng = &mut self.rngs[c];
            let threshold = &mut self.spawn_threshold[c];

            // TODO(Luke): don't unwrap here
            // CPU starts quivering...
            for (sample_in, sample_out) in chan_in.unwrap().iter().zip(chan_out.iter_mut()) {
                let mut accumulator = 0.0;
                let mut weight = 0.0;

                // Spawn a grain once every active grain is on the down ramp
                // of its window, so grains overlap and crossfade. Jitter pulls
                // the threshold earlier, never later, so it can't open gaps.
                let should_spawn_grain = !grains
                    .iter()
                    .any(|x| x.active() && x.window_phase() < (1.0 - x.get_shape()) * *threshold);

                if should_spawn_grain
                    && band_ok
                    && let Some(grain) = grains.iter_mut().find(|x| x.ready())
                {
                    let pos = (*spawn_offset + self.spray * rng.bipolar()).clamp(lo, hi);
                    grain.spawn(pos, grain_len, self.freq, self.shape);
                    *threshold = 1.0 - self.jitter * rng.unipolar();
                }

                // TODO: Audit this, adapted from other version
                for g in grains.iter_mut() {
                    if !g.active {
                        continue;
                    }

                    let env = g.get_window();

                    // TODO(Luke): how pricey is this?
                    let delayed = delay_line.read_cubic(g.sample_pos);

                    g.win_phase += g.win_phase_inc;
                    if g.win_phase >= 1.0 {
                        g.active = false;
                    }

                    g.sample_pos += 1.0 - g.sample_inc;

                    accumulator += delayed * env;
                    weight += env;
                }

                // Only ever attenuate: boosting a lone fade-in would square off its edge
                let finished = (accumulator / weight.max(1.0)).tanh();
                *sample_out = finished;

                // Add to delay line with feedback, check math here, might need different fade/normalization logic
                let with_feedback = sample_in + (finished * self.feedback);
                // Damping the feedback with a basic one-pole filter
                let filtered =
                    with_feedback * (1.0 - self.damping_a) + self.damping_state[c] * self.damping_a;

                delay_line.push(filtered);
                self.damping_state[c] = filtered;

                *spawn_offset += 1.0 - self.scan[c];

                // TODO: More algorithms
                if band_ok && (*spawn_offset < lo || *spawn_offset > hi) {
                    *spawn_offset = lo + (*spawn_offset - lo).rem_euclid(hi - lo);
                }
            }
        }
    }
}

impl GrainDelay {
    /// Give us an acceptable range to spawn a grain.
    ///
    /// For starters, we have to pad by our interpolation
    /// algorithm or else we will have discontinuities.
    fn spawn_band(&self, grain_len: f32) -> (f32, f32) {
        // Convert our pitch variable to some multiple of middle c
        // We're using middle c because this make it more "playable"
        // down the line.
        let rate = self.freq.max(1e-3) / MIDDLE_C;

        // We move 1.0 sample per tick, but we also have to factor in the rate.
        //
        // For an intuitive visualization, if we advanced one sample for every tick,
        // we would always be reading the same point in time, as this would cancel out
        // with the natural 1 sample tick from pushing to the delay line.
        let drift = (1.0 - rate) * grain_len / rate;

        // Pad our low sample by our interpolation steps and our drift
        let lo = CUBIC_NEWER + (-drift).max(0.0);

        // Do the same, but using our delay length and the other side of the interpolation.
        let hi = (self.delay_len as f32)
            .min(self.delay_capacity as f32 - 1.0 - CUBIC_OLDER - drift.max(0.0));

        (lo, hi)
    }
}

impl NodeDefinition for GrainDelay {
    const NAME: &'static str = "grain_delay";
    const DESCRIPTION: &'static str = "A basic grain delay, with no parameters yet";
    const REQUIRED_PARAMS: &'static [&'static str] = &["delay_length"];
    const OPTIONAL_PARAMS: &'static [&'static str] = &[
        "chans",
        "grain_size",
        "shape",
        "feedback",
        "damping",
        "scan",
        "jitter",
        "spray",
    ];

    fn create(
        rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        let sr = rb.config.sample_rate;

        let chans = p.get_usize("chans")?.unwrap_or(2);
        let delay_len = p.require_duration_ms("delay_length")?;

        let delay_len = (delay_len.as_secs_f32() * sr as f32).floor() as usize;

        let delay_capacity =
            (delay_len + (CUBIC_NEWER + CUBIC_OLDER) as usize + 1).next_power_of_two();

        let scan = p.get_f32("scan")?.unwrap_or(1.0);

        let damping = p.get_f32("damping")?.unwrap_or(0.6).clamp(0.0001, 0.999);

        let grain_size = p
            .get_duration_ms("grain_size")?
            .unwrap_or(Duration::from_millis(200))
            .clamp(Duration::from_millis(5), Duration::from_secs(3));

        let shape = p.get_f32("shape")?.unwrap_or(0.3);

        let feedback = p.get_f32("feedback")?.unwrap_or(0.3);

        let jitter = p.get_f32("jitter")?.unwrap_or(0.0);

        let spray = p
            .get_duration_ms("spray")?
            .map_or(0.0, |d| d.as_secs_f32() * sr as f32);

        let mut keys = Vec::new();
        for i in 0..chans {
            // TODO(Luke): I didn't think about using this yet without needing a name.
            // Fix this soon, likely breaks with multiple!!!
            let key = rb.add_delay_line(&format!("grain_delay_key_{}", (i)), delay_capacity);
            keys.push(key);
        }

        let node = GrainDelay::new(
            keys,
            delay_capacity,
            delay_len,
            damping,
            chans,
            shape,
            scan,
            grain_size,
            feedback,
            jitter,
            spray,
        );

        Ok(Box::new(node))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(delay_len: usize, capacity: usize, freq: f32) -> GrainDelay {
        let mut n = GrainDelay::new(
            vec![DelayLineKey::default()],
            capacity,
            delay_len,
            0.5,
            1,
            0.3,
            1.0,
            Duration::from_millis(100),
            0.0,
            0.0,
            0.0,
        );
        n.freq = freq;
        n
    }

    /// The spawn rule overlaps grains exactly on their tapers, so with no
    /// jitter the normalization never engages.
    #[test]
    fn scheduled_tapers_sum_to_unity() {
        let mut out = Grain::default();
        let mut inc = Grain::default();
        out.spawn(0.0, 1000.0, MIDDLE_C, 0.25);
        inc.spawn(0.0, 1000.0, MIDDLE_C, 0.25);
        for k in 0..=100 {
            let t = 0.25 * k as f32 / 100.0;
            out.win_phase = 0.75 + t;
            inc.win_phase = t;
            assert!((out.get_window() + inc.get_window() - 1.0).abs() < 1e-4);
        }
    }

    /// Every read a grain makes, from spawn to deactivation, must keep its
    /// cubic taps inside the written region of the ring buffer.
    fn assert_grain_stays_in_bounds(n: &GrainDelay, spawn_at: f32, grain_len: f32) {
        let mut g = Grain::default();
        g.spawn(spawn_at, grain_len, n.freq, n.shape);
        let max = n.delay_capacity as f32 - 1.0 - CUBIC_OLDER;
        while g.active {
            assert!(
                g.sample_pos >= CUBIC_NEWER && g.sample_pos <= max,
                "pos {} outside [{CUBIC_NEWER}, {max}]",
                g.sample_pos
            );
            g.win_phase += g.win_phase_inc;
            if g.win_phase >= 1.0 {
                g.active = false;
            }
            g.sample_pos += 1.0 - g.sample_inc;
        }
    }

    #[test]
    fn band_edges_are_safe_across_pitches() {
        let grain_len = 4800.0;
        for freq in [MIDDLE_C * 0.5, MIDDLE_C, MIDDLE_C * 1.5, MIDDLE_C * 2.0] {
            let n = node(48_000, 65_536, freq);
            let (lo, hi) = n.spawn_band(grain_len);
            assert!(hi > lo);
            assert_grain_stays_in_bounds(&n, lo, grain_len);
            assert_grain_stays_in_bounds(&n, hi, grain_len);
        }
    }

    #[test]
    fn band_is_empty_when_drift_exceeds_buffer() {
        let n = node(1_000, 1_024, MIDDLE_C * 4.0);
        let (lo, hi) = n.spawn_band(48_000.0);
        assert!(hi <= lo);
    }
}
