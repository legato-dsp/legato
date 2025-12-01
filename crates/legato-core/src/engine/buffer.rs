use core::fmt;
use core::ops::{Deref, DerefMut};
use generic_array::GenericArray;
use std::simd::{LaneCount, Simd, SupportedLaneCount};

use crate::engine::node::BufferSize;

#[derive(Clone)]
pub struct BufferWithLanes<N: BufferSize, const L: usize> {
    pub data: GenericArray<f32, N>,
}

impl<N: BufferSize, const L: usize> BufferWithLanes<N, L>
where
    LaneCount<L>: SupportedLaneCount,
{
    pub const LANES: usize = L;

    pub const CHUNKS: usize = N::USIZE / L;

    pub fn silent() -> Self {
        Self {
            data: GenericArray::default(),
        }
    }
    #[inline(always)]
    pub fn to_simd(&self) -> (&[f32], &[Simd<f32, L>], &[f32]) {
        self.data.as_slice().as_simd()
    }

    #[inline(always)]
    pub fn to_simd_mut(&mut self) -> (&mut [f32], &mut [Simd<f32, L>], &mut [f32]) {
        self.data.as_mut_slice().as_simd_mut()
    }

    #[inline(always)]
    pub fn chunk_size(&self) -> usize {
        Self::CHUNKS
    }
}

impl<N: BufferSize, const L: usize> Default for BufferWithLanes<N, L>
where
    LaneCount<L>: SupportedLaneCount,
{
    fn default() -> Self {
        Self::silent()
    }
}

impl<N: BufferSize, const L: usize> From<GenericArray<f32, N>> for BufferWithLanes<N, L>
where
    LaneCount<L>: SupportedLaneCount,
{
    fn from(data: GenericArray<f32, N>) -> Self {
        Self { data }
    }
}

impl<N: BufferSize, const L: usize> fmt::Debug for BufferWithLanes<N, L>
where
    LaneCount<L>: SupportedLaneCount,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.data.as_slice(), f)
    }
}

impl<N: BufferSize, const L: usize> PartialEq for BufferWithLanes<N, L>
where
    LaneCount<L>: SupportedLaneCount,
{
    fn eq(&self, other: &Self) -> bool {
        self.data == other.data
    }
}

impl<N: BufferSize, const L: usize> Deref for BufferWithLanes<N, L>
where
    LaneCount<L>: SupportedLaneCount,
{
    type Target = [f32];
    fn deref(&self) -> &Self::Target {
        self.data.as_slice()
    }
}

impl<N: BufferSize, const L: usize> DerefMut for BufferWithLanes<N, L>
where
    LaneCount<L>: SupportedLaneCount,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.data.as_mut_slice()
    }
}

pub type FrameWithLanes<N, const L: usize> = [BufferWithLanes<N, L>];

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

pub type Vf32 = std::simd::Simd<f32, LANES>;
pub type Vusize = std::simd::Simd<usize, LANES>; // Note: Could there be edge cases here?

pub type Buffer<N> = BufferWithLanes<N, LANES>;
pub type Frame<N> = [Buffer<N>]; // TODO: Guarantee safe lane sizes

// TODO: Lanes and latency sizes
