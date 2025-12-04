use crate::{nodes::ports::Ported, runtime::context::AudioContext};

pub mod audio;
pub mod ports;

pub type NodeInputs = [Box<[f32]>];

pub trait Node: Ported {
    fn process<'a>(
        &mut self,
        ctx: &mut AudioContext,
        ai: &NodeInputs,
        ao: &mut NodeInputs,
        ci: &NodeInputs,
        co: &mut NodeInputs,
    );
}

// pub trait Node: Ported {
//     fn process(&mut self, ctx: &mut AudioContext,
//         ai: &[ &Vec<f32> ],
//         ao: &mut[ &mut Vec<f32> ],
//         ci: &[ &Vec<f32> ],
//         co: &mut[ &mut Vec<f32>] ],
//     );
// }
