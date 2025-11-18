use std::ops::Mul;

use generic_array::ArrayLength;
use typenum::{Prod, U2};

use crate::engine::{audio_context::AudioContext, buffer::Frame, port::PortedErased};

pub trait X2: ArrayLength + Send + Sync + 'static {
    type X2;
}

pub trait FrameSize: ArrayLength + Send + Sync + 'static + X2 {
    const SIZE: usize = Self::USIZE;
}

impl<N> FrameSize for N
where
    N: ArrayLength + Send + Sync + 'static + Mul<U2>,
{
    const SIZE: usize = N::USIZE;
}

impl<N> X2 for N
where
    N: FrameSize + ArrayLength + Mul<U2> + Send + Sync + 'static,
{
    type X2 = Prod<N, U2>;
}

pub trait Node<AF, CF>: PortedErased
where
    AF: FrameSize,
    CF: FrameSize,
{
    fn process(
        &mut self,
        ctx: &mut AudioContext<AF>,
        ai: &Frame<AF>,
        ao: &mut Frame<AF>,
        ci: &Frame<CF>,
        co: &mut Frame<CF>,
    );
    fn tick_ctrl(&mut self) {}
}
