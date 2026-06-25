use std::simd::{
    Simd,
    num::{SimdFloat, SimdUint},
};

use crate::{
    math::{ONE_VIDX, cubic_hermite, cubic_hermite_simd, lerp, lerp_simd},
    resources::window::Window,
    simd::{Vf32, Vidx},
};

#[derive(Clone)]
/// A power of two delay line implementation using an underlying window into a continous resource buffer.
pub struct ResourceDelay {
    cursor: usize,
    mask: usize, // Must be capacity - 1
    window: Window,
}

impl ResourceDelay {
    pub fn new(window: Window) -> Self {
        let size = window.len;
        assert!(size.is_power_of_two(), "Buffer size must be power of 2");
        Self {
            cursor: 0,
            mask: size - 1, // Correct masking
            window,
        }
    }

    #[inline(always)]
    pub fn push(&mut self, data: &mut [f32], val: f32) {
        debug_assert_eq!(self.window.len, data.len());

        data[self.cursor] = val;
        self.cursor = (self.cursor + 1) & self.mask;
    }

    #[inline(always)]
    fn read_idx(&self, delay: usize) -> usize {
        self.cursor.wrapping_add(self.mask).wrapping_sub(delay) & self.mask
    }

    #[inline(always)]
    pub fn get_offset(&self, data: &[f32], delay_samples: usize) -> f32 {
        debug_assert_eq!(self.window.len, data.len());
        let idx = self.read_idx(delay_samples.min(self.mask));
        // SAFETY: idx is always < mask + 1 == data.len()
        unsafe { *data.get_unchecked(idx) }
    }

    #[inline(always)]
    pub fn get_delay_linear(&self, data: &[f32], offset: f32) -> f32 {
        debug_assert_eq!(self.window.len, data.len());
        let floor = offset as usize;
        let base = self.read_idx(floor.min(self.mask));
        let prev = base.wrapping_sub(1) & self.mask;
        // SAFETY: indices are masked
        let (a, b) = unsafe { (*data.get_unchecked(base), *data.get_unchecked(prev)) };
        lerp(a, b, offset - floor as f32)
    }

    #[inline(always)]
    pub fn get_delay_cubic(&self, data: &[f32], offset: f32) -> f32 {
        debug_assert_eq!(self.window.len, data.len());
        let floor = offset.floor() as usize;
        let i1 = self.read_idx(floor.min(self.mask));
        let i0 = i1.wrapping_add(1) & self.mask;
        let i2 = i1.wrapping_sub(1) & self.mask;
        let i3 = i1.wrapping_sub(2) & self.mask;
        // SAFETY: all indices are masked
        let (a, b, c, d) = unsafe {
            (
                *data.get_unchecked(i0),
                *data.get_unchecked(i1),
                *data.get_unchecked(i2),
                *data.get_unchecked(i3),
            )
        };
        cubic_hermite(a, b, c, d, offset - floor as f32)
    }

    #[inline(always)]
    pub fn get_delay_linear_simd(&self, data: &[f32], offsets: Vf32) -> Vf32 {
        debug_assert_eq!(self.window.len, data.len());

        let mask = Vidx::splat(self.mask as u32);
        let cursor = Vidx::splat(self.cursor as u32);

        let floor = offsets.simd_max(Vf32::splat(0.0)).cast::<u32>();
        let frac = offsets - floor.cast::<f32>();

        let base = (cursor + mask - floor) & mask;
        let prev = (base + mask) & mask;

        let gather = |idx: Vidx| -> Vf32 {
            Vf32::from_array(std::array::from_fn(|k| unsafe {
                *data.get_unchecked(idx.as_array()[k] as usize)
            }))
        };

        lerp_simd(gather(base), gather(prev), frac)
    }

    #[inline(always)]
    pub fn get_delay_cubic_simd(&self, data: &[f32], offsets: Vf32) -> Vf32 {
        debug_assert_eq!(self.window.len, data.len());

        let mask = Vidx::splat(self.mask as u32);
        let cursor = Vidx::splat(self.cursor as u32);

        let floor = offsets.simd_max(Vf32::splat(0.0)).cast::<u32>();
        let frac = offsets - floor.cast::<f32>();

        let i1 = (cursor + mask - floor) & mask;
        let i0 = (i1 + ONE_VIDX) & mask;
        let i2 = (i1 + mask) & mask;
        let i3 = (i1 + mask - ONE_VIDX) & mask;

        let gather = |idx: Vidx| -> Vf32 {
            Vf32::from_array(std::array::from_fn(|k| unsafe {
                *data.get_unchecked(idx.as_array()[k] as usize)
            }))
        };

        cubic_hermite_simd(gather(i0), gather(i1), gather(i2), gather(i3), frac)
    }

    #[inline(always)]
    pub fn get_window(&self) -> Window {
        self.window
    }
}

// Here, we define views that the nodes can reference directly

pub struct DelayLineView<'a> {
    pub delay: &'a ResourceDelay,
    pub data: &'a [f32],
}

pub struct DelayLineViewMut<'a> {
    pub delay: &'a mut ResourceDelay,
    pub data: &'a mut [f32],
}

impl<'a> DelayLineView<'a> {
    #[inline(always)]
    pub fn read_linear(&self, offset: f32) -> f32 {
        self.delay.get_delay_linear(self.data, offset)
    }

    #[inline(always)]
    pub fn read_cubic(&self, offset: f32) -> f32 {
        self.delay.get_delay_cubic(self.data, offset)
    }

    #[inline(always)]
    pub fn read_linear_simd(&self, offsets: Vf32) -> Vf32 {
        self.delay.get_delay_linear_simd(self.data, offsets)
    }

    #[inline(always)]
    pub fn read_cubic_simd(&self, offsets: Vf32) -> Vf32 {
        self.delay.get_delay_cubic_simd(self.data, offsets)
    }
}

impl<'a> DelayLineViewMut<'a> {
    #[inline(always)]
    pub fn push(&mut self, val: f32) {
        self.delay.push(self.data, val);
    }

    #[inline(always)]
    pub fn read_linear(&self, offset: f32) -> f32 {
        self.delay.get_delay_linear(self.data, offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::window::Window;

    #[test]
    fn test_push_and_wrap() {
        let size = 4;
        let mut buffer = vec![0.0; size];
        let mut delay = ResourceDelay::new(Window {
            len: size,
            start: 0,
        });

        delay.push(&mut buffer, 1.0);
        delay.push(&mut buffer, 2.0);
        delay.push(&mut buffer, 3.0);
        delay.push(&mut buffer, 4.0);

        // Buffer should be [1.0, 2.0, 3.0, 4.0]
        // Cursor should have wrapped back to 0
        assert_eq!(delay.cursor, 0);

        delay.push(&mut buffer, 5.0);
        assert_eq!(buffer[0], 5.0);
        assert_eq!(delay.cursor, 1);
    }

    #[test]
    fn test_get_offset_logic() {
        let size = 4;
        let mut buffer = vec![0.0; size];
        let mut delay = ResourceDelay::new(Window {
            len: size,
            start: 0,
        });

        // Push sequence: 10.0, 20.0, 30.0
        delay.push(&mut buffer, 10.0); // cursor becomes 1
        delay.push(&mut buffer, 20.0); // cursor becomes 2
        delay.push(&mut buffer, 30.0); // cursor becomes 3

        // At delay 0, we expect the most recently pushed value (30.0)
        assert_eq!(delay.get_offset(&buffer, 0), 30.0);
        assert_eq!(delay.get_offset(&buffer, 1), 20.0);
        assert_eq!(delay.get_offset(&buffer, 2), 10.0);
    }

    #[test]
    #[should_panic]
    fn test_non_power_of_two() {
        let _ = ResourceDelay::new(Window { len: 3, start: 0 });
    }

    #[test]
    fn linear_scalar_and_simd_match() {
        let size = 4096;
        let mut buffer = vec![0.0; size];
        let mut delay = ResourceDelay::new(Window {
            len: size,
            start: 0,
        });

        for i in 0..size {
            delay.push(&mut buffer, i as f32);
        }

        for i in 1..1024 {
            let off = i as f32 + 0.37;
            let scalar = delay.get_delay_linear(&buffer, off);
            let simd = delay.get_delay_linear_simd(&buffer, Vf32::splat(off));
            for lane in simd.as_array() {
                assert!(
                    (lane - scalar).abs() < 1e-4,
                    "linear SIMD mismatch: scalar={scalar}, simd={lane}, offset={off}"
                );
            }
        }
    }

    #[test]
    fn test_boundary_clamping() {
        let size = 4;
        let buffer = vec![1.0, 2.0, 3.0, 4.0];
        let delay = ResourceDelay::new(Window {
            len: size,
            start: 0,
        });

        // Requesting a delay of 100 should be clamped to the max buffer size (mask)
        let val = delay.get_offset(&buffer, 100);
        assert!(val > 0.0);
    }
}
