use std::simd::{LaneCount, Simd, StdFloat, SupportedLaneCount, num::{SimdFloat, SimdUint}};

use crate::{
    runtime::lanes::{LANES, Vf32, Vusize},
    utils::math::{ONE_VF32, ONE_VUSIZE, TWO_VUSIZE, cubic_hermite, cubic_hermite_simd, lerp, lerp_simd},
};

#[derive(Debug, Clone)]
pub struct RingBuffer {
    data: Box<[f32]>,
    capacity: usize,
    write_pos: usize,
}

impl RingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            data: vec![0.0; capacity].into(),
            capacity,
            write_pos: 0,
        }
    }

    #[inline(always)]
    pub fn push(&mut self, val: f32) {
        self.data[self.write_pos] = val;
        self.write_pos = (self.write_pos + 1) % self.capacity;
    }

    #[inline(always)]
    pub fn push_simd(&mut self, v: &Vf32) {
        let start = self.write_pos;

        if start + LANES <= self.capacity {
            // No wrap required
            self.data[start..start + LANES].copy_from_slice(v.as_array());
            self.write_pos = (start + LANES) % self.capacity;
        } else {
            // Wrap required
            let split = self.capacity - start;
            let (first, second) = v.as_array().split_at(split);

            self.data[start..self.capacity].copy_from_slice(first);
            self.data[0..second.len()].copy_from_slice(second);

            self.write_pos = second.len();
        }
    }

    #[inline(always)]
    pub fn get_offset(&self, k: usize) -> f32 {
        let len = self.capacity;
        let wp = self.write_pos;

        let idx = (wp + len - 1 - (k % len)) % len;

        self.data[idx]
    }

    pub fn get_data(&self) -> &Box<[f32]> {
        &self.data
    }

    #[inline(always)]
    /// This function grabs a chunk from the ring buffer starting at offset k,
    /// making it appear continous. Note: This may not be the right choice for
    /// rapidly modulating delay lines, rather, this is useful for algorithms like
    /// convolution.
    pub fn get_chunk_simd(&self, k: usize) -> Vf32 {
        let len = self.capacity;
        let k_mod = k % len;

        let end = (self.write_pos + len - 1 - k_mod) % len;
        let start = (end + len + 1 - LANES) % len;

        // If no wrap is required
        if start <= end && end - start + 1 == LANES {
            let slice = &self.data[start..start + LANES];
            return Vf32::from_slice(slice);
        }

        // If we wrap, we have to copy from other section
        let mut out = [0f32; LANES];

        let first_len = len - start; // Say we have 12 samples and 4 are wrapped
        let second_len = LANES - first_len; // Here we have the remainding 4

        out[..first_len].copy_from_slice(&self.data[start..]);
        out[first_len..].copy_from_slice(&self.data[..second_len]);

        Vf32::from_array(out)
    }

    pub fn clear(&mut self) {
        self.data.fill(0.0);
        self.write_pos = 0;
    }

    #[inline(always)]
    /// A utility to split the ring buffer into two slices.
    /// This is particularly useful when doing FIR
    pub fn as_slices(&self) -> (&[f32], &[f32]) {
        let head = self.write_pos;

        if head == 0 {
            (&self.data[..], &[])
        } else {
            (&self.data[head..], &self.data[..head])
        }
    }

    #[inline(always)]
    pub fn get_delay_linear(&self, offset: f32) -> f32 {
        let floor = offset as usize;

        let a = self.get_offset(floor);
        let b = self.get_offset(floor + 1);

        let t = offset - floor as f32;

        lerp(a, b, t)
    }

    #[inline(always)]
    pub fn get_delay_cubic(&self, offset: f32) -> f32 {
        let floor = offset.floor() as usize;

        let a = self.get_offset(floor.saturating_sub(1));
        let b = self.get_offset(floor);
        let c = self.get_offset(floor + 1);
        let d = self.get_offset(floor + 2);

        let t = offset - floor as f32;

        cubic_hermite(a, b, c, d, t)
    }

    #[inline(always)]
    pub fn get_delay_linear_simd(&self, offset: Vf32) -> Vf32 {
        let floor_float = offset.floor();

        let floor_usize = floor_float.cast::<usize>();

        let a = self.gather_simd(floor_usize);
        let b = self.gather_simd(floor_usize + Vusize::splat(1));

        let t = offset - floor_float;

        lerp_simd(a, b, t)
    }

    #[inline(always)]
    pub fn get_delay_cubic_simd(&self, offset: Vf32) -> Vf32 {
        let floor_float = offset.floor();

        let floor_usize = floor_float.cast::<usize>();

        let a = self.gather_simd(floor_usize.saturating_sub(ONE_VUSIZE));
        let b = self.gather_simd(floor_usize);
        let c = self.gather_simd(floor_usize + ONE_VUSIZE);
        let d = self.gather_simd(floor_usize + TWO_VUSIZE);

        let t = offset - floor_float;

        cubic_hermite_simd(a, b, c, d, t)
    }

    fn gather_simd<const N: usize>(&self, indices: Simd<usize, N>) -> Simd<f32, N> 
    where LaneCount<N>: SupportedLaneCount
    {
        // TODO: Is there a better solution?
        let mut out = [0.0; N];
        let len = self.capacity;
        let base = (self.write_pos + len - 1) % len;

        for i in 0..N {
            let k = indices[i];
            let idx = (base + len - k) % len;
            out[i] = self.data[idx];
        }

        Simd::<f32, N>::from_array(out)
    }
}



mod test {
    use std::array;

    use crate::utils::math::{one_usize_simd, two_usize_simd};

    use super::*;

    impl RingBuffer {
    // Generic function, mostly for testing
        pub fn get_delay_cubic_simd_generic<const N: usize>(&self, offset: Simd<f32, N>) -> Simd<f32, N>
        where LaneCount<N>: SupportedLaneCount
        {
            let floor_float = offset.floor();

            let floor_usize = floor_float.cast::<usize>();

            let a = self.gather_simd(floor_usize.saturating_sub(one_usize_simd()));
            let b = self.gather_simd(floor_usize);
            let c = self.gather_simd(floor_usize + one_usize_simd());
            let d = self.gather_simd(floor_usize + two_usize_simd());

            let t = offset - floor_float;

            cubic_hermite_simd(a, b, c, d, t)
        }
        pub fn get_delay_linear_simd_generic<const N: usize>(&self, offset: Simd<f32, N>) -> Simd<f32, N>
        where LaneCount<N>: SupportedLaneCount
        {
            let floor_float = offset.floor();

            let floor_usize = floor_float.cast::<usize>();

            let a = self.gather_simd(floor_usize);
            let b = self.gather_simd(floor_usize + one_usize_simd());

            let t = offset - floor_float;

            lerp_simd(a, b, t)
        }
    }

        #[test]
    fn offset_sanity() {
        let mut rb = RingBuffer::new(8);

        for i in 0..12 {
            rb.push(i as f32);
        }
        // v
        // 0 0 0 0  0 0 0 0
        // V
        // 0 1 2 3  4 5 6 7
        //           v
        // 8 9 10 11 4 5 6 7

        dbg!(rb.get_data());

        assert_eq!(rb.get_offset(0), 11.0);
        assert_eq!(rb.get_offset(1), 10.0);
        assert_eq!(rb.get_offset(7), 4.0);
    }

    #[test]
    fn test_push_chunk_no_wrap() {
        let mut rb = RingBuffer::new(32);

        let v = Vf32::from_array(array::from_fn(|x| x as f32));
        rb.push_simd(&v);

        let out = rb.get_chunk_simd(0);
        assert_eq!(out, v);
    }

    #[test]
    fn test_push_chunk_wrap() {
        let mut rb = RingBuffer::new(LANES + 2);

        // wrap the position around
        for _ in 0..(LANES + 1) {
            rb.push(1.0);
        }

        let v = Vf32::from_array(array::from_fn(|x| x as f32));
        rb.push_simd(&v);

        let out = rb.get_chunk_simd(0);
        assert_eq!(out, v);
    }

    #[test]
    fn test_get_offset_chunk_wrap_copy() {
        let mut rb = RingBuffer::new(LANES + 4);

        for i in 0..(LANES + 4) {
            rb.push(i as f32);
        }

        let chunk = rb.get_chunk_simd(1);

        let mut expected = [0f32; LANES];
        for i in 0..LANES {
            expected[LANES - 1 - i] = rb.get_offset(1 + i);
        }

        assert_eq!(chunk, Vf32::from_array(expected));
    }

    #[test]
    fn test_get_offset_chunk_two() {
        let mut rb = RingBuffer::new(LANES * 2);

        for n in 1..4 {
            rb.push_simd(&Vf32::from_array(array::from_fn(|_| n as f32)));
        }

        // v
        // 0000  0000 - we start with an empty buffer, widx at 0
        // [cnk] v
        // 1111  0000 - we write the first chunk, write id at LANES
        // [cnk]
        // v
        // 1111  2222 - write the second chunk, looping to start
        // [cnk] v
        // 3333  2222

        assert_eq!(rb.get_chunk_simd(0), Vf32::splat(3.0));
        assert_eq!(rb.get_chunk_simd(1 * LANES), Vf32::splat(2.0));
    }

    #[test]
    fn test_clear() {
        let mut rb = RingBuffer::new(16);

        rb.push(1.0);
        rb.push(2.0);
        rb.push(3.0);

        rb.clear();

        assert_eq!(rb.write_pos, 0);
        assert!(rb.data.iter().all(|&x| x == 0.0));
    }

    #[test]
    fn scalar_simd_equivalence_linear() {
        let mut rb = RingBuffer::new(4096);
        for i in 0..4096 {
            rb.push(i as f32);
        }

        for i in 0..4096 {
            let base = i;
            let simd_off = Vf32::splat(i as f32);
            let s = rb.get_delay_linear(base as f32);
            let v = rb.get_delay_linear_simd(simd_off);

            for lane in v.as_array() {
                assert!((lane - s).abs() < 1e-6);
            }
        }
    }

    #[test]
    fn scalar_simd_equivalence_cubic() {
        let mut rb = RingBuffer::new(4096);
        for i in 0..4096 {
            rb.push(i as f32);
        }

        for i in 0..1024 {
            let base = i;
            let simd_off = Vf32::splat(i as f32);
            let s = rb.get_delay_cubic(base as f32);
            let v = rb.get_delay_cubic_simd(simd_off);

            for lane in v.as_array() {
                assert!((lane - s).abs() < 1e-6);
            }
        }
    }



    #[test]
    fn test_cubic_sample_order() {
        let capacity = 32;
        let mut rb = RingBuffer::new(capacity);

        for n in 0..capacity {
            rb.push(n as f32);
        }

        let offset = 5.3_f32;
        let scalar = rb.get_delay_cubic(offset);

        let simd_offset = Vf32::splat(offset);
        let simd = rb.get_delay_cubic_simd(simd_offset).as_array()[0];

        dbg!(rb.get_data());

        let expected = 31.0 - 5.3f32;

        let allowed_error = 1e-5;

        assert!(
            (scalar - expected).abs() < allowed_error,
            "Scalar cubic interpolation WRONG ORDER: got {}, expected {}",
            scalar,
            expected
        );

        assert!(
            (simd - expected).abs() < allowed_error,
            "SIMD cubic interpolation WRONG ORDER: got {}, expected {}",
            simd,
            expected
        );
    }

    #[test]
    fn linear_scalar_simd_interp() {

        let mut rb = RingBuffer::new(8);
        for i in 0..8 {
            rb.push(i as f32);
        }

        dbg!(rb.get_data());

        let a = rb.get_delay_linear(1.0);
        let b = rb.get_delay_linear(1.5);
        
        let b_c = rb.get_delay_linear(1.8);

        let c = rb.get_delay_linear(2.0);

        assert_eq!(a, 6.0);
        assert_eq!(b, 5.5);
        assert_eq!(b_c, 5.2);
        assert_eq!(c, 5.0);

        let chunk = rb.get_delay_linear_simd_generic(std::simd::Simd::<f32, 4>::from_array([1.0, 1.5, 1.8, 2.0]));

        let chunk_arr = chunk.as_array();

        assert_eq!(a, chunk_arr[0]);
        assert_eq!(b, chunk_arr[1]);
        assert_eq!(b_c, chunk_arr[2]);
        assert_eq!(c, chunk_arr[3]);
    }
}
