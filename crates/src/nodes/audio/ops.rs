use crate::{
    context::AudioContext,
    math::fast_tanh_vf32,
    node::{Inputs, Node},
    ports::{PortBuilder, Ports},
    simd::{LANES, Vf32},
};

#[derive(Clone)]
pub struct ApplyOp {
    val: f32,
    apply_op: fn(Vf32, Vf32) -> Vf32,
    ports: Ports,
    chans: usize,
}

impl ApplyOp {
    /// Here, we typically pass in chans = 1 for binary two channel operations.
    /// For instance, multiply two streams by each other.
    ///
    /// However, we can also pass in multiple channels that are operated against
    /// the current val, which is convenient for say mc gain
    pub fn new(val: f32, chans: usize, apply_op: fn(Vf32, Vf32) -> Vf32) -> Self {
        Self {
            val,
            apply_op,
            chans,
            ports: PortBuilder::default()
                .audio_in(1)
                .audio_in_named(&["val"])
                .audio_out(1)
                .build(),
        }
    }
    fn process_no_input_val(&mut self, ai: &Inputs, ao: &mut [&mut [f32]]) {
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
    fn process_with_input_val(&mut self, val: &[f32], ai: &Inputs, ao: &mut [&mut [f32]]) {
        let audio_inputs = &ai[0..self.chans];

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
    fn process(&mut self, _: &mut AudioContext, ai: &Inputs, ao: &mut [&mut [f32]]) {
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
    // Fast soft clip
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

use crate::{
    builder::{ResourceBuilderView, ValidationError},
    dsl::ir::DSLParams,
    node::DynNode,
    spec::NodeDefinition,
};

/// Zero-size definition type for the `mult` DSL node.
pub struct MultDef;
/// Zero-size definition type for the `add` DSL node.
pub struct AddDef;
/// Zero-size definition type for the `sub` DSL node.
pub struct SubDef;
/// Zero-size definition type for the `div` DSL node.
pub struct DivDef;
/// Zero-size definition type for the `gain` DSL node.
pub struct GainDef;

impl NodeDefinition for MultDef {
    const NAME: &'static str = "mult";
    const DESCRIPTION: &'static str = "Multiplies an audio signal by a scalar or modulation input";
    const REQUIRED_PARAMS: &'static [&'static str] = &["val"];
    const OPTIONAL_PARAMS: &'static [&'static str] = &[];

    fn create(
        _rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        let val = p.get_f32("val").unwrap_or(1.0);
        Ok(Box::new(mult_node_factory(val, 1, ApplyOpKind::Mult)))
    }
}

impl NodeDefinition for AddDef {
    const NAME: &'static str = "add";
    const DESCRIPTION: &'static str = "Adds a scalar or modulation input to an audio signal";
    const REQUIRED_PARAMS: &'static [&'static str] = &["val"];
    const OPTIONAL_PARAMS: &'static [&'static str] = &[];

    fn create(
        _rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        let val = p.get_f32("val").unwrap_or(0.0);
        Ok(Box::new(mult_node_factory(val, 1, ApplyOpKind::Add)))
    }
}

impl NodeDefinition for SubDef {
    const NAME: &'static str = "sub";
    const DESCRIPTION: &'static str = "Subtracts a scalar or modulation input from an audio signal";
    const REQUIRED_PARAMS: &'static [&'static str] = &["val"];
    const OPTIONAL_PARAMS: &'static [&'static str] = &[];

    fn create(
        _rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        let val = p.get_f32("val").unwrap_or(0.0);
        Ok(Box::new(mult_node_factory(val, 1, ApplyOpKind::Subtract)))
    }
}

impl NodeDefinition for DivDef {
    const NAME: &'static str = "div";
    const DESCRIPTION: &'static str = "Divides an audio signal by a scalar or modulation input";
    const REQUIRED_PARAMS: &'static [&'static str] = &["val"];
    const OPTIONAL_PARAMS: &'static [&'static str] = &[];

    fn create(
        _rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        let val = p.get_f32("val").unwrap_or(0.0);
        Ok(Box::new(mult_node_factory(val, 1, ApplyOpKind::Div)))
    }
}

impl NodeDefinition for GainDef {
    const NAME: &'static str = "gain";
    const DESCRIPTION: &'static str =
        "Applies multichannel gain with soft clipping (tanh saturation)";
    const REQUIRED_PARAMS: &'static [&'static str] = &["val"];
    const OPTIONAL_PARAMS: &'static [&'static str] = &["chans"];

    fn create(
        _rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        let chans = p.get_usize("chans").unwrap_or(2);
        let val = p.get_f32("val").unwrap_or(1.0);
        Ok(Box::new(mult_node_factory(val, chans, ApplyOpKind::Gain)))
    }
}

#[cfg(test)]
mod test {
    use crate::{
        config::{BlockSize, Config},
        harness::build_placeholder_context,
        node::Node,
        nodes::audio::ops::{ApplyOpKind, mult_node_factory},
    };

    #[test]
    fn sanity_add_inverse() {
        const BLOCK_SIZE: usize = 64;

        let buf_one = vec![-1.0; BLOCK_SIZE];
        let buf_two_val = vec![1.0; BLOCK_SIZE];

        let mut node = mult_node_factory(1.0, 1, ApplyOpKind::Add);

        let config = Config::new(48_000, BlockSize::Block64, 2, 0);

        let mut ctx = build_placeholder_context(config);

        let inputs = [Some(buf_one.as_slice()), Some(buf_two_val.as_slice())];

        let mut output_one = [42.0; BLOCK_SIZE];

        let mut outputs = [output_one.as_mut_slice()];

        node.process(&mut ctx, &inputs, &mut outputs);

        for chan in outputs.iter() {
            for sample in chan.iter() {
                assert!(sample.abs() < 1e-6);
            }
        }
    }
}
