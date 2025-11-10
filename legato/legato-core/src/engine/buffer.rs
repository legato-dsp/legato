use core::fmt;
use core::ops::{Deref, DerefMut};
use generic_array::GenericArray;

use crate::engine::node::FrameSize;

#[derive(Clone)]
pub struct Buffer<N: FrameSize> {
    pub data: GenericArray<f32, N>,
}

impl<N: FrameSize> Buffer<N> {
    pub fn silent() -> Self {
        Self {
            data: GenericArray::default(),
        }
    }
}

impl<N: FrameSize> Default for Buffer<N> {
    fn default() -> Self {
        Self::silent()
    }
}

impl<N: FrameSize> From<GenericArray<f32, N>> for Buffer<N> {
    fn from(data: GenericArray<f32, N>) -> Self {
        Self { data }
    }
}

impl<N: FrameSize> fmt::Debug for Buffer<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.data.as_slice(), f)
    }
}

impl<N: FrameSize> PartialEq for Buffer<N> {
    fn eq(&self, other: &Self) -> bool {
        self.data == other.data
    }
}

impl<N: FrameSize> Deref for Buffer<N> {
    type Target = [f32];
    fn deref(&self) -> &Self::Target {
        self.data.as_slice()
    }
}

impl<N: FrameSize> DerefMut for Buffer<N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.data.as_mut_slice()
    }
}

pub type Frame<N> = [Buffer<N>];
