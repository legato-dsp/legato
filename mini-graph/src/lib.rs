// pub mod mini_graph;
// pub mod nodes;
// pub mod utils;

mod nodes;

use core::fmt;
use core::ops::{Deref, DerefMut};

pub type Frame<const BUFFER_SIZE: usize, const CHANNEL_COUNT: usize> = [Buffer<BUFFER_SIZE>; CHANNEL_COUNT];
#[derive(Clone, Copy)]
pub struct Buffer<const BUFFER_SIZE: usize> {
    data: [f32; BUFFER_SIZE],
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

pub trait ReadPort<const N: usize, const C: usize> {
    fn get_frame<P: Port>(&self, port: P) -> Option<&Frame<N, C>>;

    fn get_buf<P: Port>(&self, port: P, ch: usize) -> Option<&Buffer<N>>;

    fn get_sample<P: Port>(&self, port: P, ch: usize, i: usize) -> Option<f32>;
}

type Inputs<const N: usize, const C: usize> = [Frame<N, C>];

impl<const N: usize, const C: usize> ReadPort<N, C> for Inputs<N, C> {
    #[inline(always)]
    fn get_frame<P: Port>(&self, port: P) -> Option<&Frame<N, C>> {
        self.get(port.into_index())
    }

    #[inline(always)]
    fn get_buf<P: Port>(&self, port: P, ch: usize) -> Option<&Buffer<N>> {
        self.get_frame(port).map(|f| &f[ch])
    }

    #[inline(always)]
    fn get_sample<P: Port>(&self, port: P, ch: usize, i: usize) -> Option<f32> {
        self.get_buf(port, ch).map(|b| b[i])
    }
}

#[derive(Debug, PartialEq)]
pub enum PortError {
    InvalidPort
}

struct AudioContext {
    sample_rate: f32,
    frame_size: usize,
    channels: usize
}

pub trait Port {
    fn into_index(&self) -> usize;
    fn from_index(index: usize) -> Result<Self, PortError> where Self: Sized;
}

trait Node<const N: usize, const C: usize> {
    type InputPorts: Port;

    fn process(&mut self, ctx: &AudioContext, inputs: &Inputs<N, C>, output: &mut Frame<N, C>);
}
