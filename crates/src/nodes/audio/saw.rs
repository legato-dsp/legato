use std::simd::{Select, Simd, StdFloat, cmp::SimdPartialOrd};

use crate::{
    builder::{ResourceBuilderView, ValidationError},
    context::AudioContext,
    dsl::ir::DSLParams,
    msg::{NodeMessage, RtValue},
    node::{DynNode, Inputs, Node},
    nodes::audio::sine::simd_scan,
    ports::{PortBuilder, Ports},
    simd::{LANES, Vf32},
    spec::NodeDefinition,
};

/// A Bandlimited PolyBlep Saw
///
/// This uses a similar phase accumulation SIMD scan technique
/// like the sine wave.
///
/// Unlike the phaser, this can be used for synthesis purposes.
#[derive(Clone)]
pub struct Saw {
    freq: f32,
    phase: f32,
    ports: Ports,
}

impl Saw {
    pub fn new(freq: f32, chans: usize) -> Self {
        Self {
            freq,
            phase: 0.0,
            ports: PortBuilder::default()
                .audio_in_named(&["freq"])
                .audio_out(chans)
                .build(),
        }
    }

    fn process_internal_freq(&mut self, ctx: &mut AudioContext, ao: &mut [&mut [f32]]) {
        let config = ctx.get_config();
        let fs_recip = 1.0 / config.sample_rate as f32;

        let freq_v = Vf32::splat(self.freq);
        let fs_recip_v = Vf32::splat(fs_recip);
        let dt_v = Vf32::splat(self.freq * fs_recip);
        let two = Vf32::splat(2.0);
        let one = Vf32::splat(1.0);

        let n = config.block_size / LANES;

        for i in 0..n {
            let inc = simd_scan(freq_v * fs_recip_v);

            let mut phase = Simd::splat(self.phase.fract());
            phase += inc;
            self.phase = phase.as_array()[LANES - 1];

            let t = phase - phase.floor(); // wrap to [0, 1)
            let naive = two * t - one;
            let sample = naive - poly_blep_simd(t, dt_v);

            let start = i * LANES;
            let end = start + LANES;

            for chan in ao.iter_mut() {
                chan[start..end].copy_from_slice(sample.as_array());
            }
        }
    }

    fn process_external_freq(
        &mut self,
        ctx: &mut AudioContext,
        fm_in: &[f32],
        ao: &mut [&mut [f32]],
    ) {
        let config = ctx.get_config();
        let fs_recip_v = Vf32::splat(1.0 / config.sample_rate as f32);
        let two = Vf32::splat(2.0);
        let one = Vf32::splat(1.0);

        for (n, freq_chunk) in fm_in.chunks_exact(LANES).enumerate() {
            let freq = Vf32::from_slice(freq_chunk);
            let dt_v = freq * fs_recip_v; // per-lane dt, used before scan

            let inc = simd_scan(dt_v);

            let mut phase = Simd::splat(self.phase.fract());
            phase += inc;
            self.phase = phase.as_array()[LANES - 1];

            let t = phase - phase.floor();
            let naive = two * t - one;
            let sample = naive - poly_blep_simd(t, dt_v);

            let start = n * LANES;
            let end = start + LANES;

            for chan in ao.iter_mut() {
                chan[start..end].copy_from_slice(sample.as_array());
            }
        }
    }
}

impl Node for Saw {
    fn process(&mut self, ctx: &mut AudioContext, ai: &Inputs, ao: &mut [&mut [f32]]) {
        if let Some(fm_in) = ai[0] {
            self.process_external_freq(ctx, fm_in, ao);
        } else {
            self.process_internal_freq(ctx, ao);
        }
    }

    fn handle_msg(&mut self, msg: NodeMessage) {
        if let NodeMessage::SetParam(payload) = msg {
            match (payload.param_name, payload.value) {
                ("freq", RtValue::F32(val)) => self.freq = val,
                _ => unreachable!("Invalid parameter and value passed"),
            }
        }
    }

    fn ports(&self) -> &Ports {
        &self.ports
    }
}

#[inline(always)]
/// A branchless SIMD poly_blep implementation
///
/// Credit to this resource here:
/// https://www.metafunction.co.uk/post/all-about-digital-oscillators-part-2-blits-bleps
fn poly_blep_simd<const LANES: usize>(
    t: Simd<f32, LANES>,
    dt: Simd<f32, LANES>,
) -> Simd<f32, LANES> {
    let zero = Simd::splat(0.0f32);
    let one = Simd::splat(1.0f32);

    let u0 = t / dt;
    let u1 = (t - one) / dt;

    let rising = u0 + u0 - u0 * u0 - one;
    let rising_mask = u0.simd_ge(zero) & u0.simd_lt(one);

    let falling = u1 * u1 + u1 + u1 + one;
    let falling_mask = u1.simd_gt(-one) & u1.simd_le(zero);

    rising_mask.select(rising, zero) + falling_mask.select(falling, zero)
}

impl NodeDefinition for Saw {
    const NAME: &'static str = "saw";
    const DESCRIPTION: &'static str = "Sawtooth wave, PolyBLEP, suitable for synthesis";
    const REQUIRED_PARAMS: &'static [&'static str] = &["chans"];
    const OPTIONAL_PARAMS: &'static [&'static str] = &["freq"];

    fn create(
        _rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        let chans = p
            .get_usize("chans")
            .expect("Must provide chans to audio_input");

        let freq = p.get_f32("freq").unwrap_or(440.0);

        Ok(Box::new(Self::new(freq, chans)))
    }
}
