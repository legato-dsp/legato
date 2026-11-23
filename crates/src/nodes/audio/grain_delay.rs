use std::time::Duration;

use crate::{
    math::fast_tanh,
    msg::{NodeMessage, RtValue},
    node::Node,
    nodes::audio::{
        grain::{Grain, tukey},
        plate::Allpass,
    },
    ports::Ports,
    prelude::*,
    resources::DelayLineKey,
    rng::XorShift,
    simd::LANES,
};

/// The most grains that may sound at once.
pub const MAX_OVERLAP: usize = 8;

/// One spare, so a grain running a step past its slot never delays the next spawn.
const NUM_GRAINS: usize = MAX_OVERLAP + 1;

/// Grains can be pitched up to an octave either way.
///
/// This is arbitrary so feel free to make a PR if you want more
pub const MAX_PITCH_SEMITONES: f32 = 12.0;

fn semitones_to_rate(semitones: f32) -> f32 {
    (semitones.clamp(-MAX_PITCH_SEMITONES, MAX_PITCH_SEMITONES) / 12.0).exp2()
}

/// Amount of padding needed on either side for cubic interpolation,
/// we then have to use these offsets to safely spawn grains without
/// discontinuities.
const CUBIC_NEWER: f32 = 1.0;
const CUBIC_OLDER: f32 = 2.0;

/// We read a whole chunk before pushing it, so no read may reach into the
/// `LANES - 1` samples that would have been pushed earlier in the same chunk.
const MIN_READ: f32 = CUBIC_NEWER + (LANES - 1) as f32;

const MIN_GRAIN_SIZE: Duration = Duration::from_millis(5);
const MAX_GRAIN_SIZE: Duration = Duration::from_secs(3);

/// Allpass tunings borrowed from mutable instruments clouds.
const DIFFUSER_REF_SR: f32 = 32_000.0;
const DIFFUSER_GAIN: f32 = 0.625;
const DIFFUSER_TUNINGS: [[f32; 4]; 2] =
    [[126.0, 180.0, 269.0, 444.0], [151.0, 205.0, 245.0, 405.0]];

struct Diffuser {
    stages: [Allpass; 4],
}

impl Diffuser {
    fn new(tuning: [f32; 4], sr: f32) -> Self {
        // We use whole samples as interpolating an allpass loop lowpasses it
        // This was only caught by a test
        let delays = tuning.map(|d| (d * sr / DIFFUSER_REF_SR).round() as usize);
        Self {
            stages: delays.map(Allpass::new),
        }
    }

    #[inline(always)]
    fn tick(&mut self, x: f32) -> f32 {
        self.stages
            .iter_mut()
            .fold(x, |x, stage| stage.tick(x, DIFFUSER_GAIN))
    }

    fn clear(&mut self) {
        self.stages.iter_mut().for_each(Allpass::clear);
    }
}

/// Decides, sample by sample, when a channel spawns its next grain.
///
/// Free running, it fires every `interval` samples, with jitter pulling spawns earlier.
/// Given a phasor, it fires at each of `slots` even divisions of the cycle instead, with
/// jitter pushing spawns later, so grains stay locked to whatever drives the phasor.
#[derive(Clone, Copy, Debug)]
struct SpawnClock {
    timer: f32,
    last_phase: f32,
    armed: bool,
}

impl Default for SpawnClock {
    fn default() -> Self {
        Self {
            timer: 0.0,
            // Below any phase, so the first sample counts as a new slot
            last_phase: -1.0,
            armed: false,
        }
    }
}

impl SpawnClock {
    #[inline(always)]
    fn due(
        &mut self,
        phase: Option<f32>,
        slots: f32,
        interval: f32,
        jitter: f32,
        rng: &mut XorShift,
    ) -> bool {
        self.timer -= 1.0;
        let Some(phase) = phase else {
            return self.timer <= 0.0;
        };
        let slot = |p: f32| (p * slots).floor();
        // A wrap starts a new cycle even when there is only one slot per cycle
        let new_slot = slot(phase) != slot(self.last_phase) || phase < self.last_phase;
        self.last_phase = phase;
        if new_slot {
            self.armed = true;
            self.timer = interval * jitter * rng.unipolar();
        }
        self.armed && self.timer <= 0.0
    }

    /// Only called once a grain actually spawned, so a full pool retries next sample.
    #[inline(always)]
    fn spawned(&mut self, synced: bool, interval: f32, jitter: f32, rng: &mut XorShift) {
        if synced {
            self.armed = false;
        } else {
            self.timer = interval * (1.0 - jitter * rng.unipolar());
        }
    }
}

/// Samples per cycle of `phasor`, from its mean step across the block.
///
/// Steps of over half a cycle are taken as transport jumps (loops, seeks) and skipped.
fn phasor_period(phasor: &[f32]) -> Option<f32> {
    let (sum, steps) = phasor
        .windows(2)
        .map(|w| (w[1] - w[0]).rem_euclid(1.0))
        .filter(|&d| d < 0.5)
        .fold((0.0, 0), |(sum, n), d| (sum + d, n + 1));
    (sum > 0.0).then(|| steps as f32 / sum)
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
///
/// Patching a 0..1 phasor into `phase` locks grains to it: `overlap` grains spawn per
/// cycle and each lasts exactly one cycle, ignoring `grain_size`. This is how a plugin
/// syncs grains to the host tempo. The `sync` param switches this off at runtime.
pub struct GrainDelay {
    /// Keys corresponding to each channels delay line
    delay_keys: Vec<DelayLineKey>,
    /// Maximum capacity of the delay line, in samples
    delay_capacity: usize,
    /// Target delay, in samples. Must be less than delay_capacity
    delay_len: f32,
    sr: f32,
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
    /// Grain playback rate, from the `pitch` param in semitones. 2.0 is an octave up,
    /// used for shimmers and similar effects.
    rate: f32,
    /// Spawn position relative for each channel, relative to the delay line
    spawn_offset: Vec<f32>,
    /// Amount we move the spawn offset per sample, per channel
    scan: Vec<f32>,
    /// Amount of grain input that gets fed back into the delay line
    feedback: f32,
    /// Average number of grains sounding at once, 1..=MAX_OVERLAP
    overlap: f32,
    /// 0..1, how far from its scheduled time a grain may spawn
    jitter: f32,
    /// Max random spawn offset either side of the scanner, in samples
    spray: f32,
    clocks: Vec<SpawnClock>,
    /// Follow the `phase` input when it is patched
    sync: bool,
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
        pitch: f32,
        scan: f32,
        grain_size: Duration,
        feedback: f32,
        overlap: f32,
        jitter: f32,
        spray: f32,
        diffusion: f32,
        sync: bool,
        sr: f32,
    ) -> Self {
        debug_assert_eq!(delay_keys.len(), chans);

        let grains = vec![[Grain::default(); NUM_GRAINS]; chans];

        Self {
            delay_keys,
            delay_capacity,
            delay_len: delay_len as f32,
            sr,
            damping_a: damping,
            damping_state: vec![0.0; chans],
            grains,
            grain_size,
            shape,
            spawn_offset: vec![delay_len as f32; chans],
            scan: vec![scan; chans],
            rate: semitones_to_rate(pitch),
            feedback,
            overlap: overlap.clamp(1.0, MAX_OVERLAP as f32),
            jitter: jitter.clamp(0.0, 1.0),
            spray: spray.max(0.0),
            clocks: vec![SpawnClock::default(); chans],
            sync,
            rngs: (0..chans).map(|_| XorShift::seeded()).collect(),
            diffusion: diffusion.clamp(0.0, 1.0),
            diffusers: (0..chans)
                .map(|c| Diffuser::new(DIFFUSER_TUNINGS[c % 2], sr))
                .collect(),
            ports: PortBuilder::default()
                .audio_in(chans)
                .required_in()
                .default_in()
                .audio_in_named(&["phase"])
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
    fn reset(&mut self, ctx: &mut AudioContext) {
        let resources = ctx.get_resources_mut();
        for &key in &self.delay_keys {
            resources.delay_line_view_mut(key).clear();
        }
        self.grains.iter_mut().flatten().for_each(Grain::stop);
        self.damping_state.fill(0.0);
        self.spawn_offset.fill(self.delay_len);
        self.clocks.fill(SpawnClock::default());
        self.diffusers.iter_mut().for_each(Diffuser::clear);
    }
    fn process(&mut self, ctx: &mut AudioContext, inputs: &Inputs, outputs: &mut [&mut [f32]]) {
        let sr = ctx.get_config().sample_rate;
        let resources = ctx.get_resources_mut();

        let sr = sr as f32;
        let chans = self.delay_keys.len();
        let phasor = inputs.get(chans).copied().flatten().filter(|_| self.sync);
        let synced = phasor.is_some();

        let free_len = self.grain_size.as_secs_f32() * sr;
        let slots = self.overlap.round();
        let (grain_len, interval) = match phasor.and_then(phasor_period) {
            Some(period) => {
                let lifetime = self.fit_lifetime(period, sr);
                (lifetime * self.rate, lifetime / slots)
            }
            None => (free_len, self.spawn_interval(free_len)),
        };
        let (lo, hi) = self.spawn_band(grain_len);
        let band_ok = hi > lo;

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
            let clock = &mut self.clocks[c];
            let diffuser = &mut self.diffusers[c];

            // TODO(Luke): don't unwrap here
            // CPU starts quivering...
            let chunks = chan_in
                .unwrap()
                .chunks_exact(LANES)
                .zip(chan_out.chunks_exact_mut(LANES));

            for (ci, (chunk_in, chunk_out)) in chunks.enumerate() {
                // Grain scheduling never depends on the signal, so we resolve the whole
                // chunk up front.
                //
                // Breaking up this dependency helps performance as LLVM can likely get a bit more
                // aggressive here.
                let mut phase = [[0.0f32; LANES]; NUM_GRAINS];
                let mut offset = [[0.0f32; LANES]; NUM_GRAINS];
                let mut live = [false; NUM_GRAINS];

                for k in 0..LANES {
                    // Spawn our grains on a fixed clock so `overlap` grains sound at once.
                    //
                    // Note(Luke): We also add jitter here, as otherwise I was getting some nasty
                    // correlation or similar artifacts. With more grains it's probably not noticable, but
                    // it's worth noting this was an explicit decision.
                    //
                    // Something like WSOLA might be the more proper approach for this?
                    let host_phase = phasor.map(|p| p[ci * LANES + k]);
                    if clock.due(host_phase, slots, interval, self.jitter, rng)
                        && band_ok
                        && let Some(grain) = grains.iter_mut().find(|x| x.ready())
                    {
                        let pos = (*spawn_offset + self.spray * rng.bipolar()).clamp(lo, hi);
                        grain.spawn(pos, grain_len, self.rate, self.shape);
                        clock.spawned(synced, interval, self.jitter, rng);
                    }

                    for (g, grain) in grains.iter_mut().enumerate() {
                        if !grain.active {
                            continue;
                        }

                        live[g] = true;
                        phase[g][k] = grain.win_phase;
                        offset[g][k] = grain.sample_pos - k as f32;

                        grain.win_phase += grain.win_phase_inc;
                        if grain.win_phase >= 1.0 {
                            grain.active = false;
                        }

                        grain.sample_pos += 1.0 - grain.sample_inc;
                    }

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

                // Idle lanes keep a zero phase, and so a zero envelope, adding nothing.
                let mut accumulator = [0.0f32; LANES];
                let mut weight = [0.0f32; LANES];
                for g in (0..NUM_GRAINS).filter(|&g| live[g]) {
                    let shape = grains[g].shape;
                    let env = phase[g].map(|t| tukey(t, shape));
                    for k in 0..LANES {
                        let delayed = delay_line.read_cubic(offset[g][k]);
                        accumulator[k] += delayed * env[k];
                        weight[k] += env[k];
                    }
                }

                let grained: [f32; LANES] =
                    std::array::from_fn(|k| fast_tanh(accumulator[k] / weight[k].max(1.0)));

                for (k, grained) in grained.into_iter().enumerate() {
                    let finished = grained + self.diffusion * (diffuser.tick(grained) - grained);

                    chunk_out[k] = finished;

                    // Add to delay line with feedback, check math here, might need different fade/normalization logic
                    // depending on the number of grains, if they are correlating, etc.
                    let with_feedback = chunk_in[k] + (finished * self.feedback);

                    // Damping the feedback with a basic one-pole filter
                    let filtered = with_feedback * (1.0 - self.damping_a)
                        + self.damping_state[c] * self.damping_a;

                    delay_line.push(filtered);
                    self.damping_state[c] = filtered;
                }
            }
        }
    }

    fn handle_msg(&mut self, msg: NodeMessage) {
        if let NodeMessage::SetParam(p) = msg
            && let RtValue::F32(v) = p.value
        {
            match p.param_name {
                "pitch" => self.rate = semitones_to_rate(v),
                "feedback" => self.feedback = v.clamp(0.0, 1.0),
                "damping" => self.damping_a = v.clamp(0.0001, 0.999),
                "diffusion" => self.diffusion = v.clamp(0.0, 1.0),
                "shape" => self.shape = v.clamp(0.001, 0.5),
                "grain_size" => {
                    self.grain_size = Duration::from_secs_f32(v.max(0.0) / 1000.0)
                        .clamp(MIN_GRAIN_SIZE, MAX_GRAIN_SIZE)
                }
                "overlap" => self.overlap = v.clamp(1.0, MAX_OVERLAP as f32),
                "jitter" => self.jitter = v.clamp(0.0, 1.0),
                "sync" => self.sync = v >= 0.5,
                "delay_length" => self.set_delay_len(v / 1000.0 * self.sr),
                _ => {}
            }
        }
    }

    fn runtime_params(&self) -> Option<Vec<&str>> {
        Some(vec![
            "pitch",
            "feedback",
            "damping",
            "diffusion",
            "shape",
            "grain_size",
            "overlap",
            "jitter",
            "sync",
            "delay_length",
        ])
    }
}

impl GrainDelay {
    /// The oldest sample a cubic read can reach.
    fn oldest_read(&self) -> f32 {
        self.delay_capacity as f32 - 1.0 - CUBIC_OLDER
    }

    /// Moves the delay, clamped to the line's capacity. The scanners shift with it,
    /// so a scanning cloud keeps its place relative to the delay.
    fn set_delay_len(&mut self, samples: f32) {
        let delay_len = samples.clamp(MIN_READ + 1.0, self.oldest_read());
        let shift = delay_len - self.delay_len;
        self.spawn_offset.iter_mut().for_each(|o| *o += shift);
        self.delay_len = delay_len;
    }

    /// Samples between spawns. A grain lives for `grain_len / rate` samples,
    /// so spawning `overlap` times per lifetime keeps N `overlap` grains sounding.
    fn spawn_interval(&self, grain_len: f32) -> f32 {
        grain_len / self.rate / self.overlap
    }

    /// Clamps a synced grain's lifetime, in samples, so that its drift at the current
    /// rate still fits in the delay line. Past that the spawn band would be empty, and
    /// long synced grains would go silent rather than just shorter.
    fn fit_lifetime(&self, lifetime: f32, sr: f32) -> f32 {
        let lifetime = lifetime.clamp(
            MIN_GRAIN_SIZE.as_secs_f32() * sr,
            MAX_GRAIN_SIZE.as_secs_f32() * sr,
        );
        let room = self.oldest_read() - MIN_READ;
        // A lifetime of L drifts (rate - 1) * L samples through the line
        let max = 0.99 * room / (self.rate - 1.0).abs();
        lifetime.min(max)
    }

    /// Give us an acceptable range to spawn a grain.
    ///
    /// For starters, we have to pad by our interpolation
    /// algorithm or else we will have discontinuities.
    ///
    /// Then, we need to take into consideration the possible
    /// drift of the grain we are about to spawn.
    fn spawn_band(&self, grain_len: f32) -> (f32, f32) {
        let rate = self.rate;

        // We move 1.0 sample per tick, but we also have to factor in the rate.
        //
        // For an intuitive visualization, if we advanced one sample for every tick,
        // we would always be reading the same point in time, as this would cancel out
        // with the natural 1 sample tick from pushing to the delay line.
        // A grain may run a step past `grain_len / rate` before its window closes, and f32
        // accumulation of the window phase can stretch long grains a little further.
        let steps = grain_len / rate * 1.005 + 2.0;
        let drift = (1.0 - rate) * steps;

        // Pad our low sample by our interpolation steps and our drift
        let lo = MIN_READ + (-drift).max(0.0);

        // Do the same, but using our delay length and the other side of the interpolation.
        //
        // Pitched up grains read towards the write head, so they need at least `lo` of
        // history. A shorter delay is stretched to that rather than going silent.
        let hi = self
            .delay_len
            .max(lo + 1.0)
            .min(self.oldest_read() - drift.max(0.0));

        (lo, hi)
    }
}

impl NodeDefinition for GrainDelay {
    const NAME: &'static str = "grain_delay";
    const DESCRIPTION: &'static str = "A granular delay with feedback, diffusion, +-12 semitone grain pitch, and optional phasor sync";
    const REQUIRED_PARAMS: &'static [&'static str] = &["delay_length"];
    const OPTIONAL_PARAMS: &'static [&'static str] = &[
        "chans",
        "grain_size",
        "shape",
        "pitch",
        "feedback",
        "damping",
        "scan",
        "jitter",
        "spray",
        "overlap",
        "diffusion",
        "sync",
        "max_delay_length",
    ];

    fn create(
        rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        let sr = rb.config.sample_rate;

        let chans = p.get_usize("chans")?.unwrap_or(2);
        let delay_len = p.require_duration_ms("delay_length")?;

        let delay_len = (delay_len.as_secs_f32() * sr as f32).floor() as usize;

        // Room for the delay to be moved up to this at runtime
        let max_delay_len = p
            .get_duration_ms("max_delay_length")?
            .map_or(0, |d| (d.as_secs_f32() * sr as f32).floor() as usize)
            .max(delay_len);

        let delay_capacity =
            (max_delay_len + (CUBIC_NEWER + CUBIC_OLDER) as usize + 1).next_power_of_two();

        let scan = p.get_f32("scan")?.unwrap_or(1.0);

        let damping = p.get_f32("damping")?.unwrap_or(0.6).clamp(0.0001, 0.999);

        let grain_size = p
            .get_duration_ms("grain_size")?
            .unwrap_or(Duration::from_millis(200))
            .clamp(MIN_GRAIN_SIZE, MAX_GRAIN_SIZE);

        let shape = p.get_f32("shape")?.unwrap_or(0.3);

        let pitch = p.get_f32("pitch")?.unwrap_or(0.0);

        let feedback = p.get_f32("feedback")?.unwrap_or(0.3);

        let jitter = p.get_f32("jitter")?.unwrap_or(0.0);

        let spray = p
            .get_duration_ms("spray")?
            .map_or(0.0, |d| d.as_secs_f32() * sr as f32);

        let diffusion = p.get_f32("diffusion")?.unwrap_or(0.0);

        let overlap = p.get_f32("overlap")?.unwrap_or(2.0);

        let sync = p.get_bool("sync")?.unwrap_or(true);

        let mut keys = Vec::new();
        for i in 0..chans {
            // Note(Luke): this is for convenience, perhaps it does not make a ton of sense to
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
            pitch,
            scan,
            grain_size,
            feedback,
            overlap,
            jitter,
            spray,
            diffusion,
            sync,
            sr as f32,
        );

        Ok(Box::new(node))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(delay_len: usize, capacity: usize, rate: f32) -> GrainDelay {
        let mut n = GrainDelay::new(
            vec![DelayLineKey::default()],
            capacity,
            delay_len,
            0.5,
            1,
            0.3,
            0.0,
            1.0,
            Duration::from_millis(100),
            0.0,
            2.0,
            0.0,
            0.0,
            0.0,
            true,
            48_000.0,
        );
        n.rate = rate;
        n
    }

    /// Replays the spawn clock with no jitter and checks the pool never runs
    /// dry and the windows never dip below unity once the first grain is in.
    fn assert_clock_is_gapless(overlap: f32, rate: f32, shape: f32) {
        let mut n = node(48_000, 65_536, rate);
        n.overlap = overlap;
        n.shape = shape;
        let grain_len = 4800.0;
        let interval = n.spawn_interval(grain_len);

        let mut grains = [Grain::default(); NUM_GRAINS];
        let mut timer = 0.0;
        let warmup = (grain_len / rate) as usize;

        for i in 0..warmup * 4 {
            timer -= 1.0;
            if timer <= 0.0 {
                let g = grains.iter_mut().find(|g| g.ready()).expect("pool ran dry");
                g.spawn(0.0, grain_len, n.rate, n.shape);
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
            for rate in [0.5, 1.0, 2.0] {
                assert_clock_is_gapless(overlap, rate, 0.5);
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
        out.spawn(0.0, 1000.0, 1.0, 0.25);
        inc.spawn(0.0, 1000.0, 1.0, 0.25);
        for k in 0..=100 {
            let t = 0.25 * k as f32 / 100.0;
            out.win_phase = 0.75 + t;
            inc.win_phase = t;
            assert!((out.get_window() + inc.get_window() - 1.0).abs() < 1e-4);
        }
    }

    /// Every read a grain makes, from spawn to deactivation, must keep its
    /// cubic taps inside the written region of the ring buffer, and clear of
    /// anything pushed earlier in the same chunk.
    fn assert_grain_stays_in_bounds(n: &GrainDelay, spawn_at: f32, grain_len: f32) {
        let mut g = Grain::default();
        g.spawn(spawn_at, grain_len, n.rate, n.shape);
        let max = n.delay_capacity as f32 - 1.0 - CUBIC_OLDER;
        while g.active {
            assert!(
                g.sample_pos >= MIN_READ && g.sample_pos <= max,
                "pos {} outside [{MIN_READ}, {max}]",
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
    fn band_edges_are_safe_across_pitch_range() {
        let grain_len = 4800.0;
        for semitones in -12..=12 {
            let n = node(48_000, 65_536, semitones_to_rate(semitones as f32));
            let (lo, hi) = n.spawn_band(grain_len);
            assert!(hi > lo);
            assert_grain_stays_in_bounds(&n, lo, grain_len);
            assert_grain_stays_in_bounds(&n, hi, grain_len);
        }
    }

    #[test]
    fn band_is_empty_when_drift_exceeds_buffer() {
        let n = node(1_000, 1_024, 4.0);
        let (lo, hi) = n.spawn_band(48_000.0);
        assert!(hi <= lo);
    }

    #[test]
    fn pitch_spans_an_octave_either_way_and_clamps() {
        assert_eq!(semitones_to_rate(0.0), 1.0);
        assert!((semitones_to_rate(12.0) - 2.0).abs() < 1e-6);
        assert!((semitones_to_rate(-12.0) - 0.5).abs() < 1e-6);
        assert!((semitones_to_rate(7.0) - 1.498_307).abs() < 1e-5);
        assert_eq!(semitones_to_rate(40.0), semitones_to_rate(12.0));
        assert_eq!(semitones_to_rate(-40.0), semitones_to_rate(-12.0));
    }

    #[test]
    fn pitch_message_sets_rate() {
        let mut n = node(48_000, 65_536, 1.0);
        n.handle_msg(NodeMessage::SetParam(crate::msg::ParamPayload {
            param_name: "pitch",
            value: RtValue::F32(-12.0),
        }));
        assert!((n.rate - 0.5).abs() < 1e-6);
    }

    fn ramp(period: f32, len: usize, start: f32) -> Vec<f32> {
        (0..len)
            .map(|i| (start + i as f32 / period).rem_euclid(1.0))
            .collect()
    }

    #[test]
    fn phasor_period_reads_through_wraps_and_jumps() {
        let mut p = ramp(100.0, 256, 0.9);
        assert!((phasor_period(&p).unwrap() - 100.0).abs() < 0.1);

        // A loop back in the transport is a single large step, which is skipped
        p[128..]
            .iter_mut()
            .for_each(|x| *x = (*x + 0.6).rem_euclid(1.0));
        assert!((phasor_period(&p).unwrap() - 100.0).abs() < 0.1);

        assert_eq!(phasor_period(&[0.25; 64]), None);
    }

    /// Drives a clock and grain pool from a phasor, returning the spawn sample indexes
    /// and asserting the summed windows never dip below unity once warmed up.
    fn run_synced(period: f32, slots: f32, jitter: f32, rate: f32) -> Vec<usize> {
        let mut clock = SpawnClock::default();
        let mut rng = XorShift::seeded();
        let mut grains = [Grain::default(); NUM_GRAINS];
        let interval = period / slots;
        let cycles = 6;
        let phasor = ramp(period, period as usize * cycles, 0.0);
        let mut spawns = vec![];

        for (i, &phase) in phasor.iter().enumerate() {
            if clock.due(Some(phase), slots, interval, jitter, &mut rng)
                && let Some(g) = grains.iter_mut().find(|g| g.ready())
            {
                g.spawn(0.0, period * rate, rate, 0.5);
                clock.spawned(true, interval, jitter, &mut rng);
                spawns.push(i);
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

            if jitter == 0.0 && slots >= 2.0 && i > period as usize {
                assert!(weight >= 1.0 - 1e-2, "dip to {weight} at {i}");
            }
        }
        spawns
    }

    #[test]
    fn synced_grains_spawn_on_every_slot_boundary() {
        let period = 4800.0;
        for slots in 1..=MAX_OVERLAP {
            let slots = slots as f32;
            let spawns = run_synced(period, slots, 0.0, 1.0);
            assert_eq!(spawns.len(), 6 * slots as usize);
            for (n, &at) in spawns.iter().enumerate() {
                let boundary = (n as f32 * period / slots).ceil() as usize;
                assert!(
                    at.abs_diff(boundary) <= 1,
                    "slots {slots} spawn {n} at {at}, slot at {boundary}"
                );
            }
        }
    }

    #[test]
    fn synced_grains_are_gapless_across_overlaps_and_pitches() {
        for slots in 2..=MAX_OVERLAP {
            for semitones in [-12.0, 0.0, 7.0, 12.0] {
                run_synced(4800.0, slots as f32, 0.0, semitones_to_rate(semitones));
            }
        }
    }

    #[test]
    fn synced_jitter_only_delays_within_a_slot() {
        let (period, slots, jitter) = (4800.0, 4.0, 0.5);
        let slot_len = period / slots;
        let spawns = run_synced(period, slots, jitter, 1.0);
        assert_eq!(spawns.len(), 6 * slots as usize);
        for (n, &at) in spawns.iter().enumerate() {
            let boundary = n as f32 * slot_len;
            let late = at as f32 - boundary;
            assert!(
                (-1.0..=jitter * slot_len + 1.0).contains(&late),
                "spawn {n} {late} late"
            );
        }
    }

    #[test]
    fn synced_lifetimes_always_fit_the_delay_line() {
        let sr = 48_000.0;
        for semitones in -12..=12 {
            let n = node(48_000, 65_536, semitones_to_rate(semitones as f32));
            for secs in [0.001, 0.1, 0.5, 1.0, 2.0, 3.0, 10.0] {
                let lifetime = n.fit_lifetime(secs * sr, sr);
                let grain_len = lifetime * n.rate;
                let (lo, hi) = n.spawn_band(grain_len);
                assert!(hi > lo, "{semitones}st {secs}s: empty band");
                assert_grain_stays_in_bounds(&n, lo, grain_len);
                assert_grain_stays_in_bounds(&n, hi, grain_len);
            }
        }
    }

    #[test]
    fn sync_message_toggles_phasor_following() {
        let mut n = node(48_000, 65_536, 1.0);
        n.handle_msg(NodeMessage::SetParam(crate::msg::ParamPayload {
            param_name: "sync",
            value: RtValue::F32(0.0),
        }));
        assert!(!n.sync);
    }

    fn set_delay_ms(n: &mut GrainDelay, ms: f32) {
        n.handle_msg(NodeMessage::SetParam(crate::msg::ParamPayload {
            param_name: "delay_length",
            value: RtValue::F32(ms),
        }));
    }

    #[test]
    fn delay_length_message_moves_scanners_and_clamps_to_capacity() {
        let mut n = node(48_000, 65_536, 1.0);
        n.spawn_offset[0] = 40_000.0;

        set_delay_ms(&mut n, 500.0);
        assert_eq!(n.delay_len, 24_000.0);
        assert_eq!(n.spawn_offset[0], 16_000.0);

        set_delay_ms(&mut n, 10_000.0);
        assert_eq!(n.delay_len, n.oldest_read());

        set_delay_ms(&mut n, 0.0);
        assert!(n.delay_len > MIN_READ);
    }

    #[test]
    fn short_delays_pitched_up_stretch_rather_than_go_silent() {
        let grain_len = 9600.0;
        let mut n = node(48_000, 65_536, 2.0);
        set_delay_ms(&mut n, 20.0);
        let (lo, hi) = n.spawn_band(grain_len);
        assert!(hi > lo);
        assert_grain_stays_in_bounds(&n, lo, grain_len);
        assert_grain_stays_in_bounds(&n, hi, grain_len);
    }
}
