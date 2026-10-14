#[cfg(feature = "simd")]
use crate::simd::{LANES, Vf32};
use crate::{
    node::Node,
    persample::PerSampleNode,
    ports::{PortBuilder, Ports},
};

/// Map let's you map one range to another.
///
/// For instance, -1.0..1.0 -> 120.0..240.0
///
/// This is useful for making LFOs control certain ranges.
#[derive(Clone)]
pub struct Map {
    min: f32,
    max: f32,
    mapped_min: f32,
    mapped_max: f32,
    ports: Ports,
}

impl Map {
    pub fn new(range: [f32; 2], new_range: [f32; 2]) -> Self {
        Self {
            min: range[0],
            max: range[1],
            mapped_min: new_range[0],
            mapped_max: new_range[1],
            ports: PortBuilder::default().control_in(1).control_out(1).build(),
        }
    }
}

impl Node for Map {
    #[cfg(feature = "simd")]
    fn process(
        &mut self,
        _: &mut crate::context::AudioContext,
        inputs: &crate::node::Inputs,
        outputs: &mut [&mut [f32]],
    ) {
        debug_assert!(self.max >= self.min);
        debug_assert!(self.mapped_max >= self.mapped_min);

        let min = Vf32::splat(self.min);
        let max = Vf32::splat(self.max);
        let mapped_min = Vf32::splat(self.mapped_min);
        let mapped_max = Vf32::splat(self.mapped_max);

        if let Some(in_chan) = inputs[0] {
            let out_chan = &mut outputs[0];
            for (in_chunk, out_chunk) in in_chan
                .chunks_exact(LANES)
                .zip(out_chan.chunks_exact_mut(LANES))
            {
                let res =
                    map_range_simd(Vf32::from_slice(in_chunk), min, max, mapped_min, mapped_max);
                out_chunk.copy_from_slice(res.as_array());
            }
        }
    }

    #[cfg(not(feature = "simd"))]
    fn process(
        &mut self,
        _: &mut crate::context::AudioContext,
        inputs: &crate::node::Inputs,
        outputs: &mut [&mut [f32]],
    ) {
        debug_assert!(self.max >= self.min);
        debug_assert!(self.mapped_max >= self.mapped_min);

        if let Some(in_chan) = inputs[0] {
            let out_chan = &mut outputs[0];
            for (&x, out) in in_chan.iter().zip(out_chan.iter_mut()) {
                *out = map_range(x, self.min, self.max, self.mapped_min, self.mapped_max);
            }
        }
    }

    fn ports(&self) -> &Ports {
        &self.ports
    }
}

impl PerSampleNode for Map {
    fn ports(&self) -> &Ports {
        &self.ports
    }

    fn tick(&mut self, in_frame: &[Option<f32>], out_frame: &mut [f32]) {
        if let Some(x) = in_frame[0] {
            out_frame[0] = map_range(x, self.min, self.max, self.mapped_min, self.mapped_max);
        }
    }
}

fn map_range(x: f32, in_min: f32, in_max: f32, out_min: f32, out_max: f32) -> f32 {
    let original_range = in_max - in_min;
    let new_range = out_max - out_min;

    out_min + ((x - in_min) * (new_range / original_range))
}

#[cfg(feature = "simd")]
fn map_range_simd(x: Vf32, in_min: Vf32, in_max: Vf32, out_min: Vf32, out_max: Vf32) -> Vf32 {
    let original_range = in_max - in_min;
    let new_range = out_max - out_min;

    out_min + ((x - in_min) * (new_range / original_range))
}

use crate::{
    builder::{ResourceBuilderView, ValidationError},
    dsl::ir::DSLParams,
    node::DynNode,
    spec::NodeDefinition,
};

impl Map {
    pub fn from_params(
        _rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Self, ValidationError> {
        let range = p
            .get_array_f32("range")
            .expect("Must pass original range to map");
        let new_range = p
            .get_array_f32("new_range")
            .expect("Must pass new_range to map");
        assert!(range.len() == 2);
        assert!(new_range.len() == 2);
        let mut r_0 = [0.0; 2];
        let mut r_1 = [0.0; 2];
        r_0.copy_from_slice(&range[..2]);
        r_1.copy_from_slice(&new_range[..2]);
        Ok(Self::new(r_0, r_1))
    }
}

impl NodeDefinition for Map {
    const NAME: &'static str = "map";
    const DESCRIPTION: &'static str = "Maps one numeric range to another (e.g. -1..1 → 120..240)";
    const REQUIRED_PARAMS: &'static [&'static str] = &["range", "new_range"];
    const OPTIONAL_PARAMS: &'static [&'static str] = &[];

    fn create(
        rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        Ok(Box::new(Self::from_params(rb, p)?))
    }
}

#[cfg(test)]
mod scalar_tests {
    use super::map_range;

    #[test]
    fn identity_mapping() {
        for i in 0..8 {
            let x = i as f32 / 8.0;
            assert!((map_range(x, 0.0, 1.0, 0.0, 1.0) - x).abs() < 1e-6);
        }
    }

    #[test]
    fn negative_to_positive_range() {
        let x = 0.5;
        let y = map_range(x, -1.0, 1.0, 120.0, 240.0);
        assert!((y - 210.0).abs() < 1e-6);
    }
}

// Only compiled with --features simd, since that's the only time both the
// scalar fallback and the vectorized path exist in the same binary.
#[cfg(all(test, feature = "simd"))]
mod tests {
    use super::*;

    fn scalar_map(x: f32, in_min: f32, in_max: f32, out_min: f32, out_max: f32) -> f32 {
        out_min + (x - in_min) * (out_max - out_min) / (in_max - in_min)
    }

    fn assert_simd_eq(a: Vf32, b: Vf32) {
        let aa = a.as_array();
        let bb = b.as_array();

        for i in 0..LANES {
            let diff = (aa[i] - bb[i]).abs();
            assert!(diff < 1e-6, "lane {i} mismatch: {} vs {}", aa[i], bb[i]);
        }
    }

    #[test]
    fn simd_identity_mapping() {
        let x_vals: [f32; LANES] = std::array::from_fn(|i| i as f32 / LANES as f32);
        let x = Vf32::from_array(x_vals);

        let y = map_range_simd(
            x,
            Vf32::splat(0.0),
            Vf32::splat(1.0),
            Vf32::splat(0.0),
            Vf32::splat(1.0),
        );

        assert_simd_eq(x, y);
    }

    #[test]
    fn simd_negative_to_positive_range() {
        let x_vals: [f32; LANES] =
            std::array::from_fn(|i| -1.0 + 2.0 * (i as f32) / (LANES as f32 - 1.0));

        let x = Vf32::from_array(x_vals);

        let y = map_range_simd(
            x,
            Vf32::splat(-1.0),
            Vf32::splat(1.0),
            Vf32::splat(120.0),
            Vf32::splat(240.0),
        );

        let expected = Vf32::from_array(std::array::from_fn(|i| {
            scalar_map(x_vals[i], -1.0, 1.0, 120.0, 240.0)
        }));

        assert_simd_eq(y, expected);
    }

    #[test]
    fn simd_midpoint_maps_correctly() {
        let x = Vf32::splat(0.0);

        let y = map_range_simd(
            x,
            Vf32::splat(-1.0),
            Vf32::splat(1.0),
            Vf32::splat(10.0),
            Vf32::splat(20.0),
        );

        assert_simd_eq(y, Vf32::splat(15.0));
    }

    #[test]
    fn scalar_matches_simd() {
        for i in 0..LANES {
            let x = -1.0 + 2.0 * (i as f32) / (LANES as f32).max(1.0);
            let scalar = map_range(x, -1.0, 1.0, 10.0, 500.0);
            let simd = map_range_simd(
                Vf32::splat(x),
                Vf32::splat(-1.0),
                Vf32::splat(1.0),
                Vf32::splat(10.0),
                Vf32::splat(500.0),
            )
            .as_array()[0];
            assert!((scalar - simd).abs() < 1e-5, "mismatch: {scalar} vs {simd}");
        }
    }

    #[test]
    fn simd_arbitrary_values() {
        let x_vals: [f32; LANES] = std::array::from_fn(|i| (i as f32 * 0.37).sin());

        let x = Vf32::from_array(x_vals);

        let y = map_range_simd(
            x,
            Vf32::splat(-1.0),
            Vf32::splat(1.0),
            Vf32::splat(0.0),
            Vf32::splat(100.0),
        );

        let expected = Vf32::from_array(std::array::from_fn(|i| {
            scalar_map(x_vals[i], -1.0, 1.0, 0.0, 100.0)
        }));

        assert_simd_eq(y, expected);
    }
}
