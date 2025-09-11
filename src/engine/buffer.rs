use core::fmt;
use core::ops::{Deref, DerefMut};
#[derive(Clone, Copy)]
pub struct Buffer<const BUFFER_SIZE: usize> {
    pub data: [f32; BUFFER_SIZE],
}

impl<const N: usize> Buffer<N> {
    pub const SILENT: Self = Buffer { data: [0.0; N] };
}

impl<const N: usize> Default for Buffer<N> {
    fn default() -> Self {
        Self::SILENT
    }
}

impl<const N: usize> From<[f32; N]> for Buffer<N> {
    fn from(data: [f32; N]) -> Self {
        Buffer { data }
    }
}

impl<const N: usize> fmt::Debug for Buffer<N> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.data[..], f)
    }
}

impl<const N: usize> PartialEq for Buffer<N> {
    fn eq(&self, other: &Self) -> bool {
        self[..] == other[..]
    }
}

impl<const N: usize> Deref for Buffer<N> {
    type Target = [f32];
    fn deref(&self) -> &Self::Target {
        &self.data[..]
    }
}

impl<const N: usize> DerefMut for Buffer<N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data[..]
    }
}

pub type Frame<const BUFFER_SIZE: usize> = [Buffer<BUFFER_SIZE>];
