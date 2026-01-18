use crate::{
    context::AudioContext,
    math::fast_tanh_vf32,
    node::{Channels, Inputs, Node},
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
    /// Here, chans represent the incoming number of channels. We always append one more value stream to allow for modulation
    pub fn new(val: f32, chans: usize, apply_op: fn(Vf32, Vf32) -> Vf32) -> Self {
        Self {
            val,
            apply_op,
            ports: PortBuilder::default()
                .audio_in(chans)
                .audio_in_named(&["val"])
                .audio_out(chans)
                .build(),
        }
    }
    fn process_no_input_val(&mut self, ai: &Inputs, ao: &mut Channels) {
        let chunk_size = LANES;

        let val = Vf32::splat(self.val);

        for (in_channel, out_channel) in ai.iter().zip(ao.iter_mut()) {
            for (in_chunk, out_chunk) in in_channel
                .unwrap()
                .chunks_exact(chunk_size)
                .zip(out_channel.chunks_exact_mut(chunk_size))
            {
                let result = (self.apply_op)(Vf32::from_slice(in_chunk), val);
                out_chunk.copy_from_slice(result.as_array());
            }
        }
    }
    fn process_with_input_val(&mut self, val: &[f32], ai: &Inputs, ao: &mut Channels) {
        let chans = self.ports.audio_in.len() - 1; // Remove value channel to see input channels

        let audio_inputs = &ai[..chans];

        for (in_chan, out_chan) in audio_inputs.iter().zip(ao) {
            for ((in_chunk, out_chunk), val_chunk) in in_chan
                .unwrap()
                .chunks_exact(LANES)
                .zip(out_chan.chunks_exact_mut(LANES))
                .zip(val.chunks_exact(LANES))
            {
                let result =
                    (self.apply_op)(Vf32::from_slice(in_chunk), Vf32::from_slice(val_chunk));
                out_chunk.copy_from_slice(result.as_array());
            }
        }
    }
}

impl Node for ApplyOp {
    fn process(&mut self, _: &mut AudioContext, ai: &Inputs, ao: &mut Channels) {
        let val_idx = self.ports.audio_in.len() - 1;
        let val_chan = ai[val_idx];

        match val_chan {
            Some(inner) => self.process_with_input_val(inner, ai, ao),
            None => self.process_no_input_val(ai, ao),
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
    ApplyOp::new(val, chans, op)
}

#[cfg(test)]
mod test {
    use crate::{
        config::{BlockSize, Config},
        context::AudioContext,
        node::Node,
        nodes::audio::ops::{ApplyOpKind, mult_node_factory},
    };

    #[test]
    fn sanity_add_inverse() {
        const BLOCK_SIZE: usize = 64;

        let buf_one = vec![-1.0; BLOCK_SIZE];
        let buf_two_val = vec![1.0; BLOCK_SIZE];

        let mut node = mult_node_factory(1.0, 1, ApplyOpKind::Add);

        let config = Config::new(48_000, BlockSize::Block64, 2, 4);

        let mut ctx = AudioContext::new(config);

        let inputs = [Some(buf_one.as_slice()), Some(buf_two_val.as_slice())];

        let output_one = vec![42.0; BLOCK_SIZE].into();

        let mut outputs = [output_one];

        node.process(&mut ctx, &inputs, &mut outputs);

        dbg!(&outputs);

        for chan in outputs.iter() {
            for sample in chan.iter() {
                assert!(sample.abs() < 1e-6);
            }
        }
    }
}
