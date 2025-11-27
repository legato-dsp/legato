#[cfg(target_feature = "avx512f")]
pub const LANES: usize = 16;

#[cfg(all(target_feature = "avx2", not(target_feature = "avx512f")))]
pub const LANES: usize = 8;

#[cfg(all(target_feature = "sse2", not(target_feature = "avx2")))]
pub const LANES: usize = 4;

#[cfg(target_arch = "aarch64")]
pub const LANES: usize = 4;

#[cfg(all(target_arch = "arm", target_feature = "neon"))]
pub const LANES: usize = 4;

#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
pub const LANES: usize = 4;

#[cfg(not(any(
    target_feature = "avx512f",
    target_feature = "avx2",
    target_feature = "sse2",
    all(target_arch = "aarch64"),
    all(target_arch = "arm", target_feature = "neon"),
    all(target_arch = "wasm32", target_feature = "simd128"),
)))]
pub const LANES: usize = 1;
