use std::simd::{LaneCount, Simd, StdFloat, SupportedLaneCount};

use crate::runtime::lanes::{LANES, Vf32, Vusize};


#[inline(always)]
pub fn lerp(v0: f32, v1: f32, t: f32) -> f32 {
    (1.0 - t) * v0 + t * v1
}

#[inline(always)]
pub fn lerp_simd<const N: usize>(v0: Simd<f32, N>, v1: Simd<f32, N>, t: Simd<f32, N>) -> Simd<f32, N> 
where LaneCount<N>: SupportedLaneCount
{
    (Simd::<f32, N>::splat(1.0) - t) * v0 + t * v1
}

// Adapted from https://github.com/electro-smith/DaisySP/blob/master/Source/Utility/delayline.h
#[inline(always)]
pub fn cubic_hermite(xm1: f32, x0: f32, x1: f32, x2: f32, t: f32) -> f32 {
    let c = (x1 - xm1) * 0.5;
    let v = x0 - x1;
    let w = c + v;
    let a = w + v + (x2 - x0) * 0.5;
    let b_neg = w + a;

    (((a * t) - b_neg) * t + c) * t + x0
}



// Adapted from https://github.com/electro-smith/DaisySP/blob/master/Source/Utility/delayline.h
#[inline(always)]
pub fn cubic_hermite_simd<const N: usize>(xm1: Simd<f32, N>, x0: Simd<f32, N>, x1: Simd<f32, N>, x2: Simd<f32, N>, t: Simd<f32, N>) -> Simd<f32, N>
where LaneCount<N>: SupportedLaneCount
{
    let c = (x1 - xm1) * half_f32_simd();
    let v = x0 - x1;
    let w = c + v;
    let a = w + v + (x2 - x0) * half_f32_simd();
    let b_neg = w + a;

    (((a * t) - b_neg) * t + c) * t + x0
}

#[inline(always)]
pub fn fast_tanh(x: f32) -> f32 {
    let x2 = x * x;
    let x3 = x2 * x;
    let x5 = x3 * x2;

    let a = x + (0.16489087 * x3) + (0.00985468 * x5);

    a / (1.0 + (a * a)).sqrt()
}

#[inline(always)]
pub fn fast_tanh_vf32<const N: usize>(x: Simd<f32, N>) -> Simd<f32, N>
where LaneCount<N>: SupportedLaneCount
{
    let x2 = x * x;
    let x3 = x2 * x;
    let x5 = x3 * x2;

    let a = x + (Simd::<f32, N>::splat(0.16489087) * x3) + (Simd::<f32, N>::splat(0.00985468) * x5);

    a / (one_f32_simd() + (a * a)).sqrt()
}




// A few constants to avoid splats everywhere

#[inline(always)]
pub const fn zero_f32_simd<const N: usize>() -> Simd<f32, N> where LaneCount<N>: SupportedLaneCount {
    Simd::<f32, N>::splat(0.0)
}

#[inline(always)]
pub const fn one_f32_simd<const N: usize>() -> Simd<f32, N> where LaneCount<N>: SupportedLaneCount {
    Simd::<f32, N>::splat(1.0)
}

#[inline(always)]
pub const fn half_f32_simd<const N: usize>() -> Simd<f32, N> where LaneCount<N>: SupportedLaneCount {
    Simd::<f32, N>::splat(0.5)
}

#[inline(always)]
pub const fn zero_usize_simd<const N: usize>() -> Simd<usize, N> where LaneCount<N>: SupportedLaneCount {
    Simd::<usize, N>::splat(0)
}

#[inline(always)]
pub const fn one_usize_simd<const N: usize>() -> Simd<usize, N> where LaneCount<N>: SupportedLaneCount {
    Simd::<usize, N>::splat(1)
}


#[inline(always)]
pub const fn two_usize_simd<const N: usize>() -> Simd<usize, N> where LaneCount<N>: SupportedLaneCount {
    Simd::<usize, N>::splat(2)
}

pub const ZERO_VF32: Vf32 = zero_f32_simd::<LANES>();
pub const HALF_VF32: Vf32 = half_f32_simd::<LANES>();
pub const ONE_VF32: Vf32 = one_f32_simd::<LANES>();

pub const ZERO_VUSIZE: Vusize = zero_usize_simd::<LANES>();
pub const ONE_VUSIZE: Vusize = one_usize_simd::<LANES>();
pub const TWO_VUSIZE: Vusize = two_usize_simd::<LANES>();