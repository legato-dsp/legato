use std::time::Duration;

use crate::{
    node::Node,
    nodes::{
        audio::{grain::Grain, plate::allpass},
        midi::voice::MIDDLE_C,
    },
    ports::Ports,
    prelude::*,
    resources::DelayLineKey,
    ring::RingBuffer,
    rng::XorShift,
};

const NUM_GRAINS: usize = 8;

/// Amount of padding needed on either side for cubic interpolation,
/// we then have to use these offsets to safely spawn grains without
/// discontinuities.
const CUBIC_NEWER: f32 = 1.0;
const CUBIC_OLDER: f32 = 2.0;

/// Allpass tunings borrowed from mutable instruments clouds.
const DIFFUSER_REF_SR: f32 = 32_000.0;
const DIFFUSER_GAIN: f32 = 0.625;
const DIFFUSER_TUNINGS: [[f32; 4]; 2] =
    [[126.0, 180.0, 269.0, 444.0], [151.0, 205.0, 245.0, 405.0]];

struct Diffuser {
    rings: [RingBuffer; 4],
    delays: [f32; 4],
}

impl Diffuser {
    fn new(tuning: [f32; 4], sr: f32) -> Self {
        // We use whole samples as interpolating an allpass loop lowpasses it
        // This was only caught by a test
        let delays = tuning.map(|d| (d * sr / DIFFUSER_REF_SR).round());
        Self {
            rings: delays.map(|d| RingBuffer::new(d.ceil() as usize + 8)),
            delays,
        }
    }

    #[inline(always)]
    fn tick(&mut self, x: f32) -> f32 {
        self.rings
            .iter_mut()
            .zip(self.delays)
            .fold(x, |x, (ring, d)| allpass(ring, x, DIFFUSER_GAIN, d))
    }
}

// TODO: DoD and SIMD audit

/// A Granular delay implementation using [`NUM_GRAINS`] grains
/// per channel.
///
/// For the grains, we are using a Tukey window, which I like as we
/// have a nice gradient between a Hann window and a Rectangular window.
///
/// So, with a shape of 0.5, we get smooth grains fading into each other
/// via OLA, and with a shape of 0 we get a more rectangular grain, which
/// is chunky and rhythmic. I've seen grain delays that let you modulate
/// from a few more shapes, and I had some ideas of doing this using multiple
/// window algorithms and fading between them, but I am curious if there is
/// a nicer idea out there.
///
/// I'm not using WSOLA for now as it's out of scope, but I did need to add
/// jitter to prevent some sort of correlation or similar artifacts from
/// occuring.
///
/// There is a damping loop with 4 AllPass filters, and one of these tanks
/// for each channel. This was taken from mutable instruments clouds.
///
/// Currently I only have one playback algorithm, feel free to add more!
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
    /// or other affects. Not currently exposed as I am still working on the semantics.
    freq: f32,
    /// Spawn position relative for each channel, relative to the delay line
    spawn_offset: Vec<f32>,
    /// Amount we move the spawn offset per sample, per channel
    scan: Vec<f32>,
    /// Amount of grain input that gets fed back into the delay line
    feedback: f32,
    /// Average number of grains sounding at once, 1..=NUM_GRAINS
    overlap: f32,
    /// 0..1, how much earlier than scheduled a grain may spawn
    jitter: f32,
    /// Max random spawn offset either side of the scanner, in samples
    spray: f32,
    /// Per channel, samples until the next grain spawns
    spawn_timer: Vec<f32>,
    // Note(Luke): If we don't add jitter, the delays start to flutter and make strange correlations.
    // I think a WSOLA algorithm might be the *correct* approach, but this is fine for now.
    rngs: Vec<XorShift>,
    /// 0..1 dry/wet of the diffuser on the grain output
    diffusion: f32,
    /// An array of basic schroeder allpass filters inside the feedback path to create diffusion.
    diffusers: Vec<Diffuser>,
    /// Ports...
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
        overlap: f32,
        jitter: f32,
        spray: f32,
        diffusion: f32,
        sr: f32,
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
            overlap: overlap.clamp(1.0, NUM_GRAINS as f32),
            jitter: jitter.clamp(0.0, 1.0),
            spray: spray.max(0.0),
            spawn_timer: vec![0.0; chans],
            rngs: (0..chans).map(|_| XorShift::seeded()).collect(),
            diffusion: diffusion.clamp(0.0, 1.0),
            diffusers: (0..chans)
                .map(|c| Diffuser::new(DIFFUSER_TUNINGS[c % 2], sr))
                .collect(),
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
        let interval = self.spawn_interval(grain_len);

        // TODO(Luke): sample accurate modulation for various parameters
        //
        // Also worth noting I have 0 concept of what we can modulate at audio rate here
        // without potentially blowing things up, there is bound to be a way to make this
        // a beast with certain modulation inputs but even with property based testing
        // that's a huge input space. So if someone with more DSP knowledge wants to
        // throw in their two cents, it would be appreciated.
        //
        // I would like to atleast modulate grain size, shape, feedback, and diffusion.
        for (c, (chan_in, chan_out)) in inputs.iter().zip(outputs.iter_mut()).enumerate() {
            let mut delay_line = resources.delay_line_view_mut(self.delay_keys[c]);
            let spawn_offset = &mut self.spawn_offset[c];

            let grains = &mut self.grains[c];
            let rng = &mut self.rngs[c];
            let timer = &mut self.spawn_timer[c];
            let diffuser = &mut self.diffusers[c];

            // TODO(Luke): don't unwrap here
            // CPU starts quivering...
            for (sample_in, sample_out) in chan_in.unwrap().iter().zip(chan_out.iter_mut()) {
                let mut accumulator = 0.0;
                let mut weight = 0.0;

                // Spawn our grains on a fixed clock so `overlap` grains sound at once.
                //
                // Note(Luke): We also add jitter here, as otherwise I was getting some nasty
                // correlation or similar artifacts. With more grains it's probably not noticable, but
                // it's worth noting this was an explicit decision.
                //
                // Something like WSOLA might be the more proper approach for this?
                *timer -= 1.0;
                if *timer <= 0.0
                    && band_ok
                    && let Some(grain) = grains.iter_mut().find(|x| x.ready())
                {
                    let pos = (*spawn_offset + self.spray * rng.bipolar()).clamp(lo, hi);
                    grain.spawn(pos, grain_len, self.freq, self.shape);
                    *timer = interval * (1.0 - self.jitter * rng.unipolar());
                }

                // TODO: Audit this, adapted from other version
                for g in grains.iter_mut() {
                    if !g.active {
                        continue;
                    }

                    let env = g.get_window();

                    // TODO(Luke): how pricey is this?
                    // TODO(Luke): I'm curious if we could use loop fission + interior and exterior SIMD here
                    let delayed = delay_line.read_cubic(g.sample_pos);

                    g.win_phase += g.win_phase_inc;
                    if g.win_phase >= 1.0 {
                        g.active = false;
                    }

                    g.sample_pos += 1.0 - g.sample_inc;

                    accumulator += delayed * env;
                    weight += env;
                }

                let grained = (accumulator / weight.max(1.0)).tanh();
                let finished = grained + self.diffusion * (diffuser.tick(grained) - grained);

                *sample_out = finished;

                // Add to delay line with feedback, check math here, might need different fade/normalization logic
                let with_feedback = sample_in + (finished * self.feedback);

                // Damping the feedback with a basic one-pole filter
                let filtered =
                    with_feedback * (1.0 - self.damping_a) + self.damping_state[c] * self.damping_a;

                delay_line.push(filtered);
                self.damping_state[c] = filtered;

                // We always move one sample forward from the delay line push, and scan then
                // determines how much our offset needed for the next grain spawn takes
                *spawn_offset += 1.0 - self.scan[c];

                // TODO: More algorithms
                //
                // For instance ping-pong, different directions for different channels, etc.
                if band_ok && (*spawn_offset < lo || *spawn_offset > hi) {
                    *spawn_offset = lo + (*spawn_offset - lo).rem_euclid(hi - lo);
                }
            }
        }
    }
}

impl GrainDelay {
    /// Samples between spawns. A grain lives for `grain_len / rate` samples,
    /// so spawning `overlap` times per lifetime keeps N `overlap` grains sounding.
    fn spawn_interval(&self, grain_len: f32) -> f32 {
        let rate = self.freq.max(1e-3) / MIDDLE_C;
        grain_len / rate / self.overlap
    }

    /// Give us an acceptable range to spawn a grain.
    ///
    /// For starters, we have to pad by our interpolation
    /// algorithm or else we will have discontinuities.
    ///
    /// Then, we need to take into consideration the possible
    /// drift of the grain we are about to spawn.
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
        "overlap",
        "diffusion",
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

        let diffusion = p.get_f32("diffusion")?.unwrap_or(0.0);

        let overlap = p.get_f32("overlap")?.unwrap_or(2.0);

        let mut keys = Vec::new();
        for i in 0..chans {
            // Note(Luke): this is for convenience, perhaps it does not make a tone of sense to
            // have this delay line key globally accesible?
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
            overlap,
            jitter,
            spray,
            diffusion,
            sr as f32,
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
            2.0,
            0.0,
            0.0,
            0.0,
            48_000.0,
        );
        n.freq = freq;
        n
    }

    /// Replays the spawn clock with no jitter and checks the pool never runs
    /// dry and the windows never dip below unity once the first grain is in.
    fn assert_clock_is_gapless(overlap: f32, freq: f32, shape: f32) {
        let mut n = node(48_000, 65_536, freq);
        n.overlap = overlap;
        n.shape = shape;
        let grain_len = 4800.0;
        let interval = n.spawn_interval(grain_len);

        let mut grains = [Grain::default(); NUM_GRAINS];
        let mut timer = 0.0;
        let warmup = (grain_len / (freq / MIDDLE_C)) as usize;

        for i in 0..warmup * 4 {
            timer -= 1.0;
            if timer <= 0.0 {
                let g = grains.iter_mut().find(|g| g.ready()).expect("pool ran dry");
                g.spawn(0.0, grain_len, n.freq, n.shape);
                timer = interval;
            }

            let weight: f32 = grains
                .iter_mut()
                .filter(|g| g.active)
                .map(|g| {
                    let env = g.get_window();
                    g.win_phase += g.win_phase_inc;
                    if g.win_phase >= 1.0 {
                        g.active = false;
                    }
                    env
                })
                .sum();

            if i > warmup {
                assert!(weight >= 1.0 - 1e-3, "dip to {weight} at {i}");
            }
        }
    }

    #[test]
    fn spawn_clock_is_gapless_across_overlaps_and_pitches() {
        for overlap in [2.0, 3.0, 4.0, 7.5] {
            for freq in [MIDDLE_C * 0.5, MIDDLE_C, MIDDLE_C * 2.0] {
                assert_clock_is_gapless(overlap, freq, 0.5);
            }
        }
    }

    /// Allpasses pass every frequency at unity gain, so after the chain has
    /// fully responded to an impulse its total energy must match the input's.
    #[test]
    fn diffuser_preserves_impulse_energy() {
        for tuning in DIFFUSER_TUNINGS {
            let mut d = Diffuser::new(tuning, 48_000.0);
            let energy: f32 = (0..200_000)
                .map(|i| d.tick(if i == 0 { 1.0 } else { 0.0 }).powi(2))
                .sum();
            assert!((energy - 1.0).abs() < 1e-3, "energy {energy}");
        }
    }

    /// Matching rise and fall tapers are complementary, so a crossfade between
    /// two grains holds unity gain.
    #[test]
    fn complementary_tapers_sum_to_unity() {
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
