use std::simd::StdFloat;

use crate::runtime::lanes::{Vf32, Vusize};

pub fn lerp(v0: f32, v1: f32, t: f32) -> f32 {
    (1.0 - t) * v0 + t * v1
}

#[inline(always)]
pub fn lerp_vf32(v0: Vf32, v1: Vf32, t: Vf32) -> Vf32 {
    (Vf32::splat(1.0) - t) * v0 + t * v1
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

pub const ZERO_VF32: Vf32 = Vf32::splat(0.0);
pub const HALF_VF32: Vf32 = Vf32::splat(0.5);
pub const ONE_VF32: Vf32 = Vf32::splat(1.0);

pub const ONE_VUSIZE: Vusize = Vusize::splat(1);
pub const TWO_VUSIZE: Vusize = Vusize::splat(2);

// Adapted from https://github.com/electro-smith/DaisySP/blob/master/Source/Utility/delayline.h
#[inline(always)]
pub fn cubic_hermite_vf32(xm1: Vf32, x0: Vf32, x1: Vf32, x2: Vf32, t: Vf32) -> Vf32 {
    let c = (x1 - xm1) * HALF_VF32;
    let v = x0 - x1;
    let w = c + v;
    let a = w + v + (x2 - x0) * HALF_VF32;
    let b_neg = w + a;

    (((a * t) - b_neg) * t + c) * t + x0
}

#[inline]
pub fn fast_tanh(x: f32) -> f32 {
    let x2 = x * x;
    let x3 = x2 * x;
    let x5 = x3 * x2;

    let a = x + (0.16489087 * x3) + (0.00985468 * x5);

    a / (1.0 + (a * a)).sqrt()
}

#[inline(always)]
pub fn fast_tanh_vf32(x: Vf32) -> Vf32 {
    let x2 = x * x;
    let x3 = x2 * x;
    let x5 = x3 * x2;

    let a = x + (Vf32::splat(0.16489087) * x3) + (Vf32::splat(0.00985468) * x5);

    a / (Vf32::splat(1.0) + (a * a)).sqrt()
}
