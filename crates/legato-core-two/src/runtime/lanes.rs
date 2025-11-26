#[cfg(target_feature = "avx512f")]
pub const LANES: usize = 16;

#[cfg(all(target_feature = "avx2", not(target_feature = "avx512f")))]
pub const LANES: usize = 8;

#[cfg(all(target_feature = "sse2", not(target_feature = "avx2")))]
pub const LANES: usize = 4;