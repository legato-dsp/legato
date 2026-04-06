use std::simd::{
    StdFloat,
    num::{SimdFloat, SimdUint},
};

use crate::{
    math::{ONE_VUSIZE, TWO_VUSIZE, cubic_hermite, cubic_hermite_simd, lerp, lerp_simd},
    simd::{LANES, Vf32, Vusize},
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
        let floor_usize = floor_float.cast::<usize>();
        let t = offsets - floor_float;
        let a = self.gather(floor_usize);
        let b = self.gather(floor_usize + Vusize::splat(1));
        lerp_simd(a, b, t)
    }

    #[inline(always)]
    pub fn read_cubic_simd(&self, offsets: Vf32) -> Vf32 {
        let floor_float = offsets.floor();
        let floor_usize = floor_float.cast::<usize>();
        let t = offsets - floor_float;
        let a = self.gather(floor_usize.saturating_sub(ONE_VUSIZE));
        let b = self.gather(floor_usize);
        let c = self.gather(floor_usize + ONE_VUSIZE);
        let d = self.gather(floor_usize + TWO_VUSIZE);
        cubic_hermite_simd(a, b, c, d, t)
    }

    #[inline(always)]
    fn gather(&self, indices: Vusize) -> Vf32 {
        let mut out = [0.0f32; LANES];
        for lane in 0..LANES {
            unsafe {
                out[lane] = *self.data.get_unchecked(indices[lane]);
            }
        }
        Vf32::from_array(out)
    }
}
