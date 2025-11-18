use std::ops::Mul;

use generic_array::ArrayLength;
use typenum::{Prod, U0, U2};

use crate::engine::{buffer::Frame, node::FrameSize, runtime::Runtime};

pub struct Application<AF, CF, C>
where
    AF: FrameSize + Mul<U2>,
    Prod<AF, U2>: FrameSize,
    CF: FrameSize,
    C: ArrayLength,
{
    runtime: Runtime<AF, CF, C, U0>,
}
impl<AF, CF, C> Application<AF, CF, C>
where
    AF: FrameSize + Mul<U2>,
    Prod<AF, U2>: FrameSize,
    CF: FrameSize,
    C: ArrayLength,
{
    pub fn new(runtime: Runtime<AF, CF, C, U0>) -> Self {
        Self { runtime }
    }
    pub fn next_block(&mut self) -> &Frame<AF> {
        self.runtime.next_block(None)
    }
}
