use std::time::Duration;

use crate::{
    node::Node,
    nodes::{audio::grain::Grain, midi::voice::MIDDLE_C},
    ports::Ports,
    prelude::*,
    resources::{DelayLineKey, delay},
};

const NUM_GRAINS: usize = 2;

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
            freq: MIDDLE_C * 2.,
            feedback,
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

        // TODO(Luke): sample accurate modulation for various parameters
        for (c, (chan_in, chan_out)) in inputs.iter().zip(outputs.iter_mut()).enumerate() {
            let mut delay_line = resources.delay_line_view_mut(self.delay_keys[c]);
            let spawn_offset = &mut self.spawn_offset[c];
            // TODO(Luke): allocated length vs actual used length????

            let grains = &mut self.grains[c];

            // TODO(Luke): don't unwrap here
            // CPU starts quivering...
            for (sample_in, sample_out) in chan_in.unwrap().iter().zip(chan_out.iter_mut()) {
                let mut accumulator = 0.0;

                // Spawn a grain once every active grain is on the down ramp
                // of its window, so grains overlap and crossfade.
                let should_spawn_grain = !grains
                    .iter()
                    .any(|x| x.active() && x.window_phase() < 1.0 - x.get_shape());

                if should_spawn_grain && let Some(grain) = grains.iter_mut().find(|x| x.ready()) {
                    grain.spawn(
                        *spawn_offset,
                        self.grain_size.as_secs_f32() * sr as f32,
                        self.freq,
                        self.shape,
                    );
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

                    // Wrap around delay length, this might not sound good...
                    g.sample_pos = g.sample_pos + 1.0 - g.sample_inc;

                    // Kill the grain at sample end
                    if g.sample_pos >= self.delay_len as f32 {
                        g.active = false;
                    }

                    // dbg!(delayed, env);
                    // TODO(Luke): Normalize by number grains
                    accumulator += delayed * env;
                }
                // Write out
                let finished = accumulator.tanh();
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
                // TODO: I am sure this can be branchless
                // TODO: Incorrect with extremes
                if *spawn_offset <= 0.0 {
                    *spawn_offset += self.delay_len as f32;
                }
                if *spawn_offset >= self.delay_len as f32 {
                    *spawn_offset -= self.delay_len as f32;
                }
            }
        }
    }
}

impl NodeDefinition for GrainDelay {
    const NAME: &'static str = "grain_delay";
    const DESCRIPTION: &'static str = "A basic grain delay, with no parameters yet";
    const REQUIRED_PARAMS: &'static [&'static str] = &["delay_length"];
    const OPTIONAL_PARAMS: &'static [&'static str] = &["chans", "grain_size", "shape", "feedback"];

    fn create(
        rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        let sr = rb.config.sample_rate;

        let chans = p.get_usize("chans")?.unwrap_or(2);
        let delay_len = p
            .get_duration_ms("delay_length")?
            .unwrap_or(Duration::from_secs(1));

        let delay_len = (delay_len.as_secs_f32() * sr as f32).floor() as usize;

        let delay_capacity = delay_len.next_power_of_two();

        let damping = p.get_f32("damping")?.unwrap_or(0.6).clamp(0.0001, 0.999);

        let grain_size = p
            .get_duration_ms("grain_size")?
            .unwrap_or(Duration::from_millis(200))
            .clamp(Duration::from_millis(5), Duration::from_secs(3));

        let shape = p.get_f32("shape")?.unwrap_or(0.3);

        let feedback = p.get_f32("feedback")?.unwrap_or(0.3);

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
            1.5,
            grain_size,
            feedback,
        );

        Ok(Box::new(node))
    }
}
