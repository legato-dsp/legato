use generic_array::ArrayLength;

use crate::engine::port::Ports;

pub struct FIR<Ai, Ao, Ci, Co>
where
    Ai: ArrayLength,
    Ao: ArrayLength,
    Ci: ArrayLength,
    Co: ArrayLength,
{
    kernel: Vec<f32>,
    ports: Ports<Ai, Ao, Ci, Co>,
}
