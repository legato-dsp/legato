use crate::{
    builder::{ResourceBuilderView, ValidationError},
    context::AudioContext,
    dsl::ir::DSLParams,
    math::cubic_hermite,
    node::{DynNode, Inputs, Node},
    ports::{PortBuilder, Ports},
    resources::ExternalBufferKey,
    spec::NodeDefinition,
};

/// For now, we use two overlapping grains to make it continous. More fun algs. in the future!
const NUM_GRAINS: usize = 2;

/// We use this as a constant to scale the incoming stream of freq values
const MIDDLE_C: f32 = 261.625565;

// TODO: Pan, more algorithms, windows, shape, variation

#[derive(Debug, Clone, Copy, Default)]
struct Grain {
    /// Used to manually kill grains on retrig
    active: bool,
    /// An f32 read position, we use interpolation
    sample_pos: f32,
    // Increment for the sample read position, not frequency dependent
    sample_inc: f32,
    /// The length of the grain
    /// The current phase of the selected window we are using
    win_phase: f32,
    win_phase_inc: f32,
    /// The Tukey window tapering constant
    shape: f32,
}

impl Grain {
    /// Process one sample of the underlying grain
    ///
    /// active_chan_buffer is an already properly sized slice of our sample.
    /// This means that we've taken the channel, and already applied a start and end.
    ///
    /// We purposefully loop around this region, as this is what a few of my favorite
    /// granular plugins do. In the future, we may have different algorithms for how
    /// we update the position of the grains, but for now, it's a simple scan.
    fn tick(&mut self, active_chan_buffer: &[f32]) -> f32 {
        if !self.active {
            return 0.0;
        }

        let env = self.get_window();

        let len = active_chan_buffer.len();

        if self.win_phase > 1.0 {
            self.active = false;
            return 0.0;
        }

        self.win_phase += self.win_phase_inc;

        // TODO: Evaluate this, might have flipped it as I adapted it from delay line
        let floor = self.sample_pos.floor() as usize;

        let i1 = floor % len;
        let i0 = (i1 + len - 1) % len;
        let i2 = i1.wrapping_add(1) % len;
        let i3 = i1.wrapping_add(2) % len;

        let (a, b, c, d) = (
            active_chan_buffer[i0],
            active_chan_buffer[i1],
            active_chan_buffer[i2],
            active_chan_buffer[i3],
        );

        let t = self.sample_pos - floor as f32; // Get the fractional remainder

        // Increment sample read and wrap to start
        self.sample_pos = (self.sample_pos + self.sample_inc) % len as f32;

        // Return the interpolated sample * our current envelope
        cubic_hermite(a, b, c, d, t) * env
    }

    /// Tukey window, we can ease between a rectangular window and a
    /// hann window, so we can control how obvious the grains are.
    ///
    /// TODO: We may want to have a small LUT, that we reupdate
    /// whenever we change the alpha.
    ///
    /// https://en.wikipedia.org/wiki/Window_function#Tukey_window
    #[inline(always)]
    fn get_window(&self) -> f32 {
        let t = self.win_phase;
        // Effectively, we are seeing what ratio of the input and output are windowed
        let a = self.shape.clamp(0.001, 0.5);

        // Ramp in
        if t < a {
            0.5 * (1.0 - (std::f32::consts::PI * (t / a - 1.0)).cos())
        }
        // Ramp out
        else if t > 1.0 - a {
            0.5 * (1.0 - (std::f32::consts::PI * ((1.0 - t) / a - 1.0)).cos())
        }
        // Rectangular path
        else {
            1.0
        }
    }

    #[inline(always)]
    fn ready(&self) -> bool {
        !self.active
    }

    #[inline(always)]
    fn active(&self) -> bool {
        self.active
    }

    #[inline(always)]
    fn stop(&mut self) {
        self.active = false;
    }

    #[inline(always)]
    fn window_phase(&self) -> f32 {
        self.win_phase
    }

    #[inline(always)]
    fn get_shape(&self) -> f32 {
        self.shape
    }

    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    fn spawn(&mut self, sample_pos: f32, sample_len: usize, freq: f32, shape: f32) {
        self.win_phase = 0.0;
        self.win_phase_inc = (freq / MIDDLE_C) / sample_len as f32;

        self.sample_pos = sample_pos;
        // TODO: In the future, this is an interesting source of musical jitter probably
        self.sample_inc = 1.0;

        self.shape = shape.clamp(0.001, 0.5);
        self.active = true;
    }
}

/// A granular sample player.
///
/// In the future, this logic should be able to be reused for a granular delay.
#[derive(Clone)]
pub struct Granular {
    /// Used to query our underlying sample
    sample_key: ExternalBufferKey,
    /// We store per channel groups of grains
    grains: Vec<[Grain; NUM_GRAINS]>,
    freq: f32,
    /// from 0.001 to 0.5, controls how rectangular (0) -> Hann shaped (0.5) the grains are
    shape: f32,
    sample_pos: f32,
    sample_start: Option<usize>,
    sample_end: Option<usize>,
    sample_increment: f32,
    ports: Ports,
}

impl Granular {
    pub fn new(
        sample_key: ExternalBufferKey,
        sample_start: Option<usize>,
        sample_end: Option<usize>,
        chans: usize,
    ) -> Self {
        // Just two per chan for now
        let grains = vec![[Grain::default(); NUM_GRAINS]; chans];

        Self {
            sample_key,
            grains,
            freq: MIDDLE_C,
            shape: 0.5,
            sample_pos: 0.0,
            sample_start,
            sample_end,
            sample_increment: 1.0,
            ports: PortBuilder::default()
                .audio_in_named(&["trig", "freq"])
                .audio_out(chans)
                .build(),
        }
    }
}

impl Node for Granular {
    fn ports(&self) -> &Ports {
        &self.ports
    }
    fn process(&mut self, ctx: &mut AudioContext, inputs: &Inputs, outputs: &mut [&mut [f32]]) {
        let sample = ctx.get_resources().get_external_buffer(self.sample_key);
        let block_size = ctx.get_config().block_size;

        let trig_chan = inputs[0].expect("No trig channel found for granular synth!");
        let freq_chan = inputs[1].expect("No freq channel found for granular synth!"); // TODO: Path with and without modulation

        if let Some(sample) = sample {
            let sample_len = sample.len();
            let end = self.sample_end.unwrap_or(sample_len).min(sample_len);
            let start = self.sample_start.unwrap_or(0).min(end);

            // Bail if zero sized
            if start == end {
                return;
            };

            for i in 0..block_size {
                let trig = trig_chan[i] == 1.0;

                // Go through all channels and grains and stop them
                if trig {
                    self.sample_pos = self.sample_start.unwrap_or(0) as f32;
                    for chan in self.grains.iter_mut() {
                        for grain in chan {
                            grain.stop();
                        }
                    }
                }

                self.freq = freq_chan[i];

                // NOTE: This logic will have to change for future granular algorithms
                for streams in &mut self.grains {
                    // Spawn a grain if we don't have any active grains, not on the down ramp of the window.
                    // The window is here because we want to spawn these with overlap.
                    let should_spawn_grain = !streams
                        .iter()
                        .any(|x| x.active() && x.window_phase() < 1.0 - x.get_shape());

                    if should_spawn_grain {
                        // Spawn the first ready grain
                        let grain = streams.iter_mut().find(|x| x.ready());
                        if let Some(inner) = grain {
                            inner.spawn(self.sample_pos, sample_len, self.freq, self.shape);
                        }
                    }
                }

                for (c, chan) in outputs.iter_mut().enumerate() {
                    let sample_chan = sample.channel(c);
                    let active_slice = &sample_chan[start..end];

                    let mut destination_sample = 0.0;

                    for g in self.grains[c].iter_mut() {
                        destination_sample += g.tick(active_slice);
                        chan[i] = destination_sample
                    }
                }

                self.sample_pos =
                    (self.sample_pos + self.sample_increment).clamp(start as f32, end as f32);
            }
        }
    }
}

impl NodeDefinition for Granular {
    const NAME: &'static str = "grain";
    const DESCRIPTION: &'static str = "A basic granular synth";
    const REQUIRED_PARAMS: &'static [&'static str] = &["sampler_name"];
    const OPTIONAL_PARAMS: &'static [&'static str] = &["chans"];

    fn create(
        rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        let name = p
            .get_str("sampler_name")
            .expect("Could not find required parameter sampler_name");
        let chans = p.get_usize("chans").unwrap_or(2);

        let key = rb.add_external_buffer_key(&name);

        Ok(Box::new(Granular::new(key, None, None, chans)))
    }
}
