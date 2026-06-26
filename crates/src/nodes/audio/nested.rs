use crate::{
    context::AudioContext,
    math::cubic_hermite,
    node::{Inputs, Node},
    ports::{PortBuilder, Ports},
};

/// Maximum nesting depth. Bounds the per-sample scratch so the hot loop needs
/// no allocation.
const MAX_NEST: usize = 8;

/// A nested allpass filter (a.k.a. Gardner allpass, FunDSP's `allnest`).
///
/// A plain Schroeder allpass has a single delay element in its loop. A *nested*
/// allpass replaces that delay element with another allpass, recursively. With
/// `delays`/`feedbacks` given outermost-first, the structure is:
///
/// ```text
/// outer allpass (delays[K-1], feedbacks[K-1])
///   └─ delay element is: inner allpass (delays[K-2], feedbacks[K-2])
///        └─ ... innermost is a plain Schroeder allpass (delays[0], feedbacks[0])
/// ```
///
/// The whole thing is still allpass (flat magnitude response) when every
/// `|feedback| < 1`, but it produces a far denser echo pattern than a single
/// section — the building block of Gardner's room reverbs and the diffusion
/// stages of plate reverbs. Because each level keeps its own delay (≥ 1 sample),
/// there is no delay-free loop, so it runs sample-accurately inside one node
/// (which is the point: the block-rate graph can't express a tight allpass nest
/// via shared delay lines).
///
/// All channels and nesting levels share one flat backing allocation, laid out
/// channel-major then level-major; each `(channel, level)` keeps a small ring
/// cursor over its slice.
#[derive(Clone)]
pub struct NestedAllpass {
    /// Single flat allocation for every `(channel, level)` ring.
    data: Box<[f32]>,
    /// Per `(channel, level)` ring state, indexed `c * k + l`.
    levels: Vec<Level>,
    /// Delay length per level, in samples (shared across channels).
    delays_samples: Vec<f32>,
    /// Allpass coefficient per level (shared across channels).
    gains: Vec<f32>,
    /// Nesting depth.
    k: usize,
    chans: usize,
    ports: Ports,
}

#[derive(Clone)]
struct Level {
    /// Offset of this ring within `data`.
    base: usize,
    /// Ring capacity (power of two).
    cap: usize,
    /// `cap - 1`, for wrapping.
    mask: usize,
    /// Next write position within the ring.
    cursor: usize,
}

impl NestedAllpass {
    pub fn new(chans: usize, delays_samples: Vec<f32>, gains: Vec<f32>) -> Self {
        let k = delays_samples.len();
        assert_eq!(gains.len(), k, "delays and feedbacks must match in length");
        assert!((1..=MAX_NEST).contains(&k), "nesting depth must be 1..={MAX_NEST}");

        let gains: Vec<f32> = gains.into_iter().map(|g| g.clamp(-0.98, 0.98)).collect();

        // A few guard samples above the delay so the cubic taps (floor+2) never
        // alias onto the freshest write.
        let caps: Vec<usize> = delays_samples
            .iter()
            .map(|d| ((*d as usize) + 4).next_power_of_two().max(4))
            .collect();
        let sum_caps: usize = caps.iter().sum();

        let data = vec![0.0; chans * sum_caps].into_boxed_slice();

        let mut levels = Vec::with_capacity(chans * k);
        for c in 0..chans {
            let chan_base = c * sum_caps;
            let mut prefix = 0;
            for &cap in &caps {
                levels.push(Level {
                    base: chan_base + prefix,
                    cap,
                    mask: cap - 1,
                    cursor: 0,
                });
                prefix += cap;
            }
        }

        Self {
            data,
            levels,
            delays_samples,
            gains,
            k,
            chans,
            ports: PortBuilder::default()
                .audio_in(chans)
                .audio_out(chans)
                .build(),
        }
    }

    /// Cubic read of one ring, `offset` samples back from the freshest write.
    /// Mirrors [`crate::ring::RingBuffer::get_delay_cubic`].
    #[inline(always)]
    fn read_cubic(data: &[f32], base: usize, cap: usize, mask: usize, cursor: usize, offset: f32) -> f32 {
        let floor = offset.floor() as usize;
        let get = |k: usize| -> f32 {
            let idx = (cursor + cap - 1 - (k & mask)) & mask;
            data[base + idx]
        };
        let a = get(floor.saturating_sub(1));
        let b = get(floor);
        let c = get(floor + 1);
        let d = get(floor + 2);
        cubic_hermite(a, b, c, d, offset - floor as f32)
    }
}

impl Node for NestedAllpass {
    fn process(&mut self, _: &mut AudioContext, inputs: &Inputs, outputs: &mut [&mut [f32]]) {
        let k = self.k;

        for c in 0..self.chans {
            let input = inputs[c].unwrap();
            let output = &mut outputs[c];

            // Hoist this channel's per-level ring state into stack scratch.
            let mut base = [0usize; MAX_NEST];
            let mut cap = [0usize; MAX_NEST];
            let mut mask = [0usize; MAX_NEST];
            let mut cur = [0usize; MAX_NEST];
            for l in 0..k {
                let lvl = &self.levels[c * k + l];
                base[l] = lvl.base;
                cap[l] = lvl.cap;
                mask[l] = lvl.mask;
                cur[l] = lvl.cursor;
            }

            let mut delayed = [0.0f32; MAX_NEST];
            let mut u = [0.0f32; MAX_NEST];

            for n in 0..input.len() {
                // Forward pass, outermost -> innermost: each level reads its
                // delay and forms the input to the level nested inside it.
                let mut signal = input[n];
                for l in (0..k).rev() {
                    let d = Self::read_cubic(&self.data, base[l], cap[l], mask[l], cur[l], self.delays_samples[l]);
                    delayed[l] = d;
                    u[l] = signal + self.gains[l] * d;
                    signal = u[l];
                }

                // Innermost (plain Schroeder allpass): store its loop signal,
                // emit its allpass output.
                self.data[base[0] + cur[0]] = u[0];
                cur[0] = (cur[0] + 1) & mask[0];
                let mut ret = delayed[0] - self.gains[0] * u[0];

                // Unwind outward: each outer level stores the inner level's
                // output into its own delay, then forms its allpass output.
                for l in 1..k {
                    self.data[base[l] + cur[l]] = ret;
                    cur[l] = (cur[l] + 1) & mask[l];
                    ret = delayed[l] - self.gains[l] * u[l];
                }

                output[n] = ret;
            }

            for l in 0..k {
                self.levels[c * k + l].cursor = cur[l];
            }
        }
    }
    fn ports(&self) -> &Ports {
        &self.ports
    }
}

use crate::{
    builder::{ResourceBuilderView, ValidationError},
    dsl::ir::DSLParams,
    node::DynNode,
    spec::NodeDefinition,
};

impl NodeDefinition for NestedAllpass {
    const NAME: &'static str = "nested_allpass";
    const DESCRIPTION: &'static str =
        "Nested (Gardner) allpass: each level's delay element is the allpass nested inside it";
    const REQUIRED_PARAMS: &'static [&'static str] = &["delays", "feedbacks", "chans"];
    const OPTIONAL_PARAMS: &'static [&'static str] = &[];

    fn create(
        rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        let chans = p.get_usize("chans").unwrap_or(2);

        let delays = p.get_array_duration_ms("delays").ok_or_else(|| {
            ValidationError::MissingRequiredParameter("nested_allpass requires `delays`".into())
        })?;
        let gains = p.get_array_f32("feedbacks").ok_or_else(|| {
            ValidationError::MissingRequiredParameter("nested_allpass requires `feedbacks`".into())
        })?;

        if delays.len() != gains.len() {
            return Err(ValidationError::InvalidParameter(
                "nested_allpass: `delays` and `feedbacks` must be the same length".into(),
            ));
        }
        if delays.is_empty() || delays.len() > MAX_NEST {
            return Err(ValidationError::InvalidParameter(format!(
                "nested_allpass: nesting depth must be 1..={MAX_NEST}"
            )));
        }

        let sr = rb.get_config().sample_rate as f32;
        let delays_samples: Vec<f32> = delays
            .iter()
            .map(|d| (sr * d.as_secs_f32()).max(1.0))
            .collect();

        Ok(Box::new(Self::new(chans, delays_samples, gains)))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        config::{BlockSize, Config},
        harness::build_placeholder_context,
    };

    /// A nested allpass is still an allpass: bounded, stable, and energy-
    /// preserving (no blow-up) for a white-noise input.
    #[test]
    fn nested_allpass_is_stable_and_bounded() {
        use rand::{Rng, SeedableRng, rngs::StdRng};

        const BLOCK: usize = 64;
        let config = Config::new(48_000, BlockSize::Block64, 2, 0);
        let mut ctx = build_placeholder_context(config);

        // Two levels: inner (small) nested inside outer (larger).
        let mut node = NestedAllpass::new(1, vec![37.0, 149.0], vec![0.7, 0.5]);

        let mut rng = StdRng::seed_from_u64(42);
        let mut in_energy = 0.0f64;
        let mut out_energy = 0.0f64;

        for _ in 0..512 {
            let input: Vec<f32> = (0..BLOCK).map(|_| rng.random_range(-1.0..1.0)).collect();
            let inputs = [Some(input.as_slice())];
            let mut out = [0.0f32; BLOCK];
            let mut outputs = [out.as_mut_slice()];

            node.process(&mut ctx, &inputs, &mut outputs);

            for &s in &input {
                in_energy += (s as f64) * (s as f64);
            }
            for &s in out.iter() {
                assert!(s.is_finite(), "non-finite output {s}");
                out_energy += (s as f64) * (s as f64);
            }
        }

        // Allpass preserves energy in steady state; allow a generous band for
        // the finite-length transient.
        let ratio = out_energy / in_energy;
        assert!(
            (0.5..1.5).contains(&ratio),
            "nested allpass energy ratio {ratio} out of bounds"
        );
    }
}
