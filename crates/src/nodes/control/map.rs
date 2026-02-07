use crate::{
    node::Node,
    ports::{PortBuilder, Ports},
    simd::{LANES, Vf32},
};

/// Map let's you map one range to another.
///
/// For instance, -1.0..1.0 -> 120.0..240.0
///
/// This is useful for making LFOs control certain ranges.
#[derive(Clone)]
pub struct Map {
    min: Vf32,
    max: Vf32,
    mapped_min: Vf32,
    mapped_max: Vf32,
    ports: Ports,
}

impl Map {
    pub fn new(range: [f32; 2], new_range: [f32; 2]) -> Self {
        Self {
            min: Vf32::splat(range[0]),
            max: Vf32::splat(range[1]),
            mapped_min: Vf32::splat(new_range[0]),
            mapped_max: Vf32::splat(new_range[1]),
            ports: PortBuilder::default().audio_in(1).audio_out(1).build(),
        }
    }
}

impl Node for Map {
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
            for (in_chunk, out_chunk) in in_chan
                .chunks_exact(LANES)
                .zip(out_chan.chunks_exact_mut(LANES))
            {
                let res = map_range_simd(
                    Vf32::from_slice(in_chunk),
                    self.min,
                    self.max,
                    self.mapped_min,
                    self.mapped_max,
                );
                out_chunk.copy_from_slice(res.as_array());
            }
        }
    }

    fn ports(&self) -> &Ports {
        &self.ports
    }
}

fn map_range_simd(x: Vf32, in_min: Vf32, in_max: Vf32, out_min: Vf32, out_max: Vf32) -> Vf32 {
    let original_range = in_max - in_min;
    let new_range = out_max - out_min;

    

    out_min + ((x - in_min) * (new_range / original_range))
}

#[cfg(test)]
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
