use std::time::Duration;

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

/// For now, we use two overlapping grains to make it continuous. More fun algs. in the future!
const NUM_GRAINS: usize = 2;

/// We use this as a constant to scale the incoming stream of freq values
const MIDDLE_C: f32 = 261.625565;

// TODO: Pan, more algorithms, windows, variation

#[derive(Debug, Clone, Copy, Default)]
struct Grain {
    /// Used to manually kill grains on retrig
    active: bool,
    /// Read position *relative to the active region*, interpolated
    sample_pos: f32,
    sample_inc: f32,
    win_phase: f32,
    win_phase_inc: f32,
    /// Tukey tapering constant, 0.001 (rectangular) to 0.5 (Hann)
    shape: f32,
}

impl Grain {
    /// Process one sample of the underlying grain.
    ///
    /// active_chan_buffer is an already properly sized slice of our sample
    /// (channel selected, start/end applied). We purposefully loop around
    /// this region, as a few of my favorite granular plugins do.
    fn tick(&mut self, active_chan_buffer: &[f32]) -> f32 {
        if !self.active {
            return 0.0;
        }

        let env = self.get_window();
        let len = active_chan_buffer.len();

        let floor = self.sample_pos.floor() as usize;
        let i1 = floor % len;
        let i0 = (i1 + len - 1) % len;
        let i2 = (i1 + 1) % len;
        let i3 = (i1 + 2) % len;

        let (a, b, c, d) = (
            active_chan_buffer[i0],
            active_chan_buffer[i1],
            active_chan_buffer[i2],
            active_chan_buffer[i3],
        );

        let t = self.sample_pos - floor as f32;
        let sample = cubic_hermite(a, b, c, d, t) * env;

        self.win_phase += self.win_phase_inc;
        if self.win_phase >= 1.0 {
            self.active = false;
        }
        self.sample_pos = (self.sample_pos + self.sample_inc) % len as f32;

        sample
    }

    /// Tukey window: eases between rectangular (a -> 0) and Hann (a = 0.5).
    ///
    /// TODO: We may want a small LUT, re-updated whenever alpha changes.
    ///
    /// https://en.wikipedia.org/wiki/Window_function#Tukey_window
    #[inline(always)]
    fn get_window(&self) -> f32 {
        let t = self.win_phase;
        let a = self.shape;

        if t < a {
            0.5 * (1.0 - (std::f32::consts::PI * t / a).cos())
        } else if t > 1.0 - a {
            0.5 * (1.0 - (std::f32::consts::PI * (1.0 - t) / a).cos())
        } else {
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

    /// grain_len_samples is the length at unison (freq == MIDDLE_C); the
    /// effective length scales inversely with pitch, so each grain always
    /// covers the same span of source material.
    #[inline(always)]
    fn spawn(&mut self, sample_pos: f32, grain_len_samples: f32, freq: f32, shape: f32) {
        self.win_phase = 0.0;
        self.sample_inc = freq.max(1e-3) / MIDDLE_C;
        self.win_phase_inc = self.sample_inc / grain_len_samples;
        self.sample_pos = sample_pos;
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
    grain_size: Duration,
    /// from 0.001 to 0.5, controls how rectangular (0) -> Hann shaped (0.5) the grains are
    shape: f32,
    /// Spawn position relative to the active region
    sample_pos: f32,
    sample_start: Option<usize>,
    sample_end: Option<usize>,
    /// Region samples advanced per output sample; 1.0 scans at original speed
    scan: f32,
    last_trig: f32,
    ports: Ports,
}

impl Granular {
    pub fn new(
        sample_key: ExternalBufferKey,
        sample_start: Option<usize>,
        sample_end: Option<usize>,
        grain_size: Duration,
        shape: f32,
        scan: f32,
        chans: usize,
    ) -> Self {
        let grains = vec![[Grain::default(); NUM_GRAINS]; chans];

        Self {
            sample_key,
            grains,
            freq: MIDDLE_C,
            grain_size,
            shape,
            sample_pos: 0.0,
            sample_start,
            sample_end,
            scan,
            last_trig: 0.0,
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
        let cfg = ctx.get_config();

        let block_size = cfg.block_size;
        let sr = cfg.sample_rate;

        let trig_chan = inputs[0].expect("No trig channel found for granular synth!");
        let freq_chan = inputs[1].expect("No freq channel found for granular synth!"); // TODO: Path with and without modulation

        let Some(sample) = sample else { return };

        let sample_len = sample.len();
        let end = self.sample_end.unwrap_or(sample_len).min(sample_len);
        let start = self.sample_start.unwrap_or(0).min(end);

        // Bail if zero sized
        if start == end {
            return;
        }
        let region_len = (end - start) as f32;

        let grain_len_samples = self.grain_size.as_secs_f32() * sr as f32;

        for i in 0..block_size {
            let trig = trig_chan[i] >= 0.5 && self.last_trig < 0.5;
            self.last_trig = trig_chan[i];

            if trig {
                self.sample_pos = 0.0;
                for chan in self.grains.iter_mut() {
                    for grain in chan {
                        grain.stop();
                    }
                }
            }

            self.freq = freq_chan[i];

            // NOTE: This logic will have to change for future granular algorithms
            for streams in &mut self.grains {
                // Spawn a grain once every active grain is on the down ramp
                // of its window, so grains overlap and crossfade.
                let should_spawn_grain = !streams
                    .iter()
                    .any(|x| x.active() && x.window_phase() < 1.0 - x.get_shape());

                if should_spawn_grain {
                    if let Some(grain) = streams.iter_mut().find(|x| x.ready()) {
                        grain.spawn(self.sample_pos, grain_len_samples, self.freq, self.shape);
                    }
                }
            }

            for (c, chan) in outputs.iter_mut().enumerate() {
                let sample_chan = sample.channel(c);
                let active_slice = &sample_chan[start..end];

                let mut destination_sample = 0.0;
                for g in self.grains[c].iter_mut() {
                    destination_sample += g.tick(active_slice);
                }

                chan[i] = destination_sample;
            }

            self.sample_pos = (self.sample_pos + self.scan).rem_euclid(region_len);
        }
    }
}

impl NodeDefinition for Granular {
    const NAME: &'static str = "grain";
    const DESCRIPTION: &'static str = "A basic granular synth";
    const REQUIRED_PARAMS: &'static [&'static str] = &["sampler_name"];
    const OPTIONAL_PARAMS: &'static [&'static str] = &["chans", "size", "scan", "shape"];

    fn create(
        rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        let name = p
            .get_str("sampler_name")
            .expect("Could not find required parameter sampler_name");

        let chans = p.get_usize("chans").unwrap_or(2);

        let grain_size = p
            .get_duration_ms("size")
            .unwrap_or(Duration::from_millis(200))
            .clamp(Duration::from_millis(5), Duration::from_secs(3));

        let shape = p.get_f32("shape").unwrap_or(0.3);

        let scan = p.get_f32("scan").unwrap_or(1.0);

        let key = rb.add_external_buffer_key(&name);

        let node = Granular::new(key, None, None, grain_size, shape, scan, chans);

        Ok(Box::new(node))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tukey_window_ramps_in_and_out() {
        let mut g = Grain::default();
        g.spawn(0.0, 100.0, MIDDLE_C, 0.25);

        // Starts at ~0, half-way up the ramp at a/2, flat in the middle, ~0 at the end
        assert!(g.get_window() < 1e-3);
        g.win_phase = 0.125;
        assert!((g.get_window() - 0.5).abs() < 1e-3);
        g.win_phase = 0.5;
        assert_eq!(g.get_window(), 1.0);
        g.win_phase = 0.999_999;
        assert!(g.get_window() < 1e-2);
    }

    #[test]
    fn grain_deactivates_after_one_window() {
        let buf = vec![0.0f32; 64];
        let mut g = Grain::default();
        g.spawn(0.0, 16.0, MIDDLE_C, 0.5);

        for _ in 0..17 {
            g.tick(&buf);
        }
        assert!(!g.active());
    }

    #[test]
    fn grain_length_scales_inversely_with_pitch() {
        let mut g = Grain::default();
        g.spawn(0.0, 100.0, MIDDLE_C * 2.0, 0.5);
        assert!((g.win_phase_inc - 0.02).abs() < 1e-6);
    }
}
