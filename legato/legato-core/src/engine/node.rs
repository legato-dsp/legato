use generic_array::ArrayLength;

use crate::engine::{audio_context::AudioContext, buffer::Frame, port::PortedErased};

pub trait FrameSize: ArrayLength + Send + Sync + 'static {
    const SIZE: usize = Self::USIZE;
}

impl<N> FrameSize for N
where
    N: ArrayLength + Send + Sync + 'static,
{
    const SIZE: usize = N::USIZE;
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
