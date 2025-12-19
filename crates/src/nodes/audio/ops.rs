use crate::{
    context::AudioContext,
    math::fast_tanh_vf32,
    node::{Channels, Node},
    ports::{PortBuilder, Ports},
    simd::{LANES, Vf32},
};

#[derive(Clone)]
pub struct ApplyOp {
    val: f32,
    apply_op: fn(Vf32, Vf32) -> Vf32,
    ports: Ports,
}

impl ApplyOp {
    pub fn new(val: f32, chans: usize, apply_op: fn(Vf32, Vf32) -> Vf32) -> Self {
        Self {
            val,
            apply_op,
            ports: PortBuilder::default()
                .audio_in(chans)
                .audio_out(chans)
                .build(),
        }
    }
}

impl Node for ApplyOp {
    fn process(
        &mut self,
        _: &mut AudioContext,
        ai: &Channels,
        ao: &mut Channels,
        
        
    ) {
        let chunk_size = LANES;

        // TODO: Automation for value
        let val = Vf32::splat(self.val);

        for (in_channel, out_channel) in ai.iter().zip(ao.iter_mut()) {
            for (in_chunk, out_chunk) in in_channel
                .chunks_exact(chunk_size)
                .zip(out_channel.chunks_exact_mut(chunk_size))
            {
                let result = (self.apply_op)(Vf32::from_slice(in_chunk), val);
                out_chunk.copy_from_slice(result.as_array());
            }
        }
    }
    fn ports(&self) -> &Ports {
        &self.ports
    }
}

fn add(a: Vf32, b: Vf32) -> Vf32 {
    a + b
}

fn subtract(a: Vf32, b: Vf32) -> Vf32 {
    a - b
}

fn mult(a: Vf32, b: Vf32) -> Vf32 {
    a * b
}

fn gain(a: Vf32, b: Vf32) -> Vf32 {
    fast_tanh_vf32(a * b)
}

fn div(a: Vf32, b: Vf32) -> Vf32 {
    a / b
}

pub enum ApplyOpKind {
    Add,
    Subtract,
    Mult,
    Div,
    Gain,
}

pub fn mult_node_factory(val: f32, chans: usize, op_kind: ApplyOpKind) -> ApplyOp {
    let op = match op_kind {
        ApplyOpKind::Add => add,
        ApplyOpKind::Subtract => subtract,
        ApplyOpKind::Mult => mult,
        ApplyOpKind::Gain => gain,
        ApplyOpKind::Div => div,
    };
    ApplyOp {
        val,
        apply_op: op,
        ports: PortBuilder::default()
            .audio_in(chans)
            .audio_out(chans)
            .build(),
    }
}
