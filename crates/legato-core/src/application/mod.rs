use std::ops::Mul;

use typenum::{Prod, U0, U2};

use crate::engine::{node::FrameSize, runtime::Runtime};

pub struct Application<AF, CF>
where
    AF: FrameSize + Mul<U2>,
    Prod<AF, U2>: FrameSize,
    CF: FrameSize,
{
    runtime: Runtime<AF, CF, U0, U0>,
}
