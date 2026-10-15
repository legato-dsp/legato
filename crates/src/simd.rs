//! Various SIMD lane size constants used when the "simd" feature is enabled

#[cfg(feature = "simd")]
#[cfg(target_feature = "avx512f")]
pub const LANES: usize = 16;

#[cfg(feature = "simd")]
#[cfg(all(target_feature = "avx2", not(target_feature = "avx512f")))]
pub const LANES: usize = 8;

#[cfg(feature = "simd")]
#[cfg(all(target_feature = "sse2", not(target_feature = "avx2")))]
pub const LANES: usize = 4;

#[cfg(feature = "simd")]
#[cfg(target_arch = "aarch64")]
pub const LANES: usize = 4;

#[cfg(feature = "simd")]
#[cfg(all(target_arch = "arm", target_feature = "neon"))]
pub const LANES: usize = 4;

#[cfg(feature = "simd")]
#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
pub const LANES: usize = 4;

#[cfg(feature = "simd")]
#[cfg(not(any(
    target_feature = "avx512f",
    target_feature = "avx2",
    target_feature = "sse2",
    all(target_arch = "aarch64"),
    all(target_arch = "arm", target_feature = "neon"),
    all(target_arch = "wasm32", target_feature = "simd128"),
)))]
pub const LANES: usize = 1;

/// Without the `simd` feature, there is no vector width: everything is
/// processed one sample at a time.
#[cfg(not(feature = "simd"))]
pub const LANES: usize = 1;

#[cfg(feature = "simd")]
pub type Vf32 = std::simd::Simd<f32, LANES>;
#[cfg(feature = "simd")]
pub type Vidx = std::simd::Simd<u32, LANES>;
