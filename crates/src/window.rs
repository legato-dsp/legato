use std::simd::{
    StdFloat,
    num::{SimdFloat, SimdUint},
};

use crate::{
    math::{ONE_VIDX, TWO_VIDX, cubic_hermite, cubic_hermite_simd, lerp, lerp_simd},
    simd::{LANES, Vf32, Vidx},
};

/// It became clear after a while, that delay lines,
/// samplers, granular, etc. need the same underlying
/// abstraction, a window into a slice, with a few fractional
/// indexing utilities.
///
/// This primative can be used to make delay lines, samplers, etc.
///
/// This also allows us to handle all of these resources as views
/// into one giant continous buffer, which should give better cache locality.
pub struct Window<'a> {
    data: &'a [f32],
}

impl<'a> Window<'a> {
    /// Construct directly from a slice. The slice should already be
    /// positioned so that index 0 is the first logical sample.
    #[inline(always)]
    pub fn new(data: &'a [f32]) -> Self {
        Self { data }
    }

    #[inline(always)]
    pub fn read_linear(&self, offset: f32) -> f32 {
        let i = offset as usize;
        let t = offset - i as f32;
        unsafe {
            let a = *self.data.get_unchecked(i);
            let b = *self.data.get_unchecked(i + 1);
            lerp(a, b, t)
        }
    }

    #[inline(always)]
    pub fn read_cubic(&self, offset: f32) -> f32 {
        let i = offset as usize;
        let t = offset - i as f32;
        unsafe {
            let p0 = *self.data.get_unchecked(i.wrapping_sub(1));
            let p1 = *self.data.get_unchecked(i);
            let p2 = *self.data.get_unchecked(i + 1);
            let p3 = *self.data.get_unchecked(i + 2);
            cubic_hermite(p0, p1, p2, p3, t)
        }
    }

    #[inline(always)]
    pub fn read_linear_simd(&self, offsets: Vf32) -> Vf32 {
        let floor_float = offsets.floor();
        let floor_idx = floor_float.cast::<u32>();
        let t = offsets - floor_float;
        let a = self.gather(floor_idx);
        let b = self.gather(floor_idx + ONE_VIDX);
        lerp_simd(a, b, t)
    }

    #[inline(always)]
    pub fn read_cubic_simd(&self, offsets: Vf32) -> Vf32 {
        let floor_float = offsets.floor();
        let floor_idx = floor_float.cast::<u32>();
        let t = offsets - floor_float;
        let a = self.gather(floor_idx.saturating_sub(ONE_VIDX));
        let b = self.gather(floor_idx);
        let c = self.gather(floor_idx + ONE_VIDX);
        let d = self.gather(floor_idx + TWO_VIDX);
        cubic_hermite_simd(a, b, c, d, t)
    }

    #[inline(always)]
    fn gather(&self, indices: Vidx) -> Vf32 {
        let mut out = [0.0f32; LANES];
        for lane in 0..LANES {
            unsafe {
                out[lane] = *self.data.get_unchecked(indices[lane] as usize);
            }
        }
        Vf32::from_array(out)
    }
}
