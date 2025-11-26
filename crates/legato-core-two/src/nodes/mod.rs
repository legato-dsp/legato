use crate::{nodes::ports::Ported, runtime::{context::AudioContext}};

pub mod ports;
pub mod audio;

pub trait Node: Ported {
    fn process(&mut self, ctx: &mut AudioContext, 
        ai: &[ &[f32] ],
        ao: &mut[ &mut[f32] ],
        ci: &[ &[f32] ],
        co: &mut[ &mut[f32] ],
    );
}