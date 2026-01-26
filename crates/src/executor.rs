use std::mem::MaybeUninit;

use slotmap::SecondaryMap;

use crate::{
    context::AudioContext,
    graph::{AudioGraph, GraphError},
    node::Inputs,
    runtime::NodeKey,
};

pub(crate) const MAX_ARITY: usize = 32;

/// For the time being, we just check if it has been prepared or not,
/// but in the future we might pause, stop, etc.
#[derive(Clone, Debug, PartialEq, Default)]
pub(crate) enum ExecutorState {
    Prepared,
    #[default]
    Unprepared,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Executor {
    data: Box<[f32]>,
    scratch: Box<[f32]>,
    pub(crate) graph: AudioGraph,
    node_offsets: SecondaryMap<NodeKey, usize>,
    // Keys for inputs/output nodes
    source_key: Option<NodeKey>,
    sink_key: Option<NodeKey>,
    state: ExecutorState,
}

impl Executor {
    /// Set the sink key for the runtime
    pub(crate) fn set_sink(&mut self, key: NodeKey) -> Result<(), GraphError> {
        match self.graph.exists(key) {
            true => {
                self.sink_key = Some(key);
                Ok(())
            }
            false => Err(GraphError::NodeDoesNotExist),
        }
    }

    /// Set the source key for the runtime
    pub(crate) fn set_source(&mut self, key: NodeKey) -> Result<(), GraphError> {
        match self.graph.exists(key) {
            true => {
                self.source_key = Some(key);
                Ok(())
            }
            false => Err(GraphError::NodeDoesNotExist),
        }
    }

    pub fn sink(&self) -> &Option<NodeKey> {
        &self.sink_key
    }

    /// Prepare the flat buffer allocation for the graph, as well as the node offsets.
    ///
    /// NOTE: This is not realtime safe!
    pub(crate) fn prepare(&mut self, block_size: usize) {
        // Allocate flat buffer
        let num_ports = self.graph.total_ports();
        let buffer_size = num_ports * block_size;

        self.data = vec![0.0; buffer_size].into();

        // Scratch buffer that gets passed for node inputs

        let scratch_len = block_size * MAX_ARITY;

        self.scratch = vec![0.0; scratch_len].into();

        // Now, we get all the keys from the topo sorted order, so we can give each node an offset into the flat buffer.
        let keys = self
            .graph
            .invalidate_topo_sort()
            .expect("Invalid graph topology found in prepare!");

        let mut total_ports = 0_usize;

        self.node_offsets.clear();

        for key in keys {
            self.node_offsets.insert(key, total_ports * block_size);

            let arity = self
                .graph
                .get_node(key)
                .unwrap()
                .get_node()
                .ports()
                .audio_out
                .len();

            total_ports += arity;
        }

        self.state = ExecutorState::Prepared;
    }

    #[inline(always)]
    pub(crate) fn process(
        &mut self,
        mut ctx: &mut AudioContext,
        external_inputs: Option<&Inputs>,
    ) -> &[&[f32]] {
        assert!(self.state == ExecutorState::Prepared);

        let block_size = ctx.get_config().block_size;

        let (sorted_order, nodes, incoming) = self.graph.get_sort_order_nodes_and_runtime_info(); // TODO: I don't like this, feels like incorrect ownership

        for node_key in sorted_order {
            let ports = nodes[*node_key].get_node().ports();

            let audio_inputs_size = ports.audio_in.len();
            let audio_outputs_size = ports.audio_out.len();

            // Fill the scratch buffer for the first N channels
            self.scratch[..audio_inputs_size * block_size].fill(0.0);

            let mut inputs: [Option<&[f32]>; MAX_ARITY] = [None; MAX_ARITY];

            let mut has_inputs: [bool; MAX_ARITY] = [false; MAX_ARITY];

            // TODO: External inputs

            let valid_inputs = self.source_key.is_some()
                && self.source_key.unwrap() == *node_key
                && external_inputs.as_ref().is_some();

            if valid_inputs {
                let ai = external_inputs.unwrap();

                for (c, chan) in ai.iter().flat_map(|x| *x).enumerate() {
                    let start = c * block_size;
                    let end = start + block_size;

                    assert_eq!(chan.len(), block_size);

                    self.scratch[start..end].copy_from_slice(chan);
                }
            } else {
                let incoming = incoming
                    .get(*node_key)
                    .expect("Invalid connection in executor!");

                for conn in incoming {
                    let base_offset = self
                        .node_offsets
                        .get(conn.source.node_key)
                        .expect("Could not find offset for node!");

                    let offset = (conn.source.port_index * block_size) + base_offset;
                    let end = offset + block_size;

                    let buffer = &self.data[offset..end];

                    has_inputs[conn.sink.port_index] = true;

                    // TODO: Zero copy for only one dependency?

                    let scratch_start = conn.sink.port_index * block_size;
                    let scratch_end = scratch_start + block_size;

                    self.scratch[scratch_start..scratch_end].copy_from_slice(buffer);
                }
            }

            for i in 0..audio_inputs_size {
                if has_inputs[i] {
                    // Pass references to each slice to the inputs now
                    let start = i * block_size;
                    let end = start + block_size;
                    inputs[i] = Some(&self.scratch[start..end]);
                }
            }

            let node = nodes
                .get_mut(*node_key)
                .expect("Could not find node at index {node_index:?}")
                .get_node_mut();

            let node_start = *self.node_offsets.get(*node_key).unwrap();

            let outputs =
                slice_node_ports_mut(&mut self.data, node_start, block_size, audio_outputs_size);

            node.process(&mut ctx, &inputs[0..audio_inputs_size], outputs);
        }

        ctx.set_instant();

        let sink_key = self.sink_key.expect("Sink node must be provided");

        let node_offset = self
            .node_offsets
            .get(sink_key)
            .expect("Could not find sink");

        let node_arity = self
            .graph
            .get_node(sink_key)
            .expect("Could not find sink")
            .get_node()
            .ports()
            .audio_out
            .len();

        let final_outputs = slice_node_ports(&self.data, *node_offset, block_size, node_arity);

        final_outputs
    }
}

#[inline(always)]
fn slice_node_ports_mut<'a>(
    buffer: &'a mut [f32],
    offset: usize,
    block_size: usize,
    chans: usize,
) -> &'a mut [&'a mut [f32]] {
    let end = (block_size * chans) + offset;

    let node_buffer = &mut buffer[offset..end];

    let slices = node_buffer.chunks_exact_mut(block_size);

    assert_eq!(slices.len(), chans);

    let mut outputs_raw: [MaybeUninit<&mut [f32]>; MAX_ARITY] =
        unsafe { MaybeUninit::uninit().assume_init() };

    for (i, slice) in slices.enumerate() {
        outputs_raw[i] = MaybeUninit::new(slice);
    }

    // TODO: Evaluate safety!
    let outputs: &mut [&mut [f32]] = unsafe {
        &mut *(&mut outputs_raw[..chans] as *mut [MaybeUninit<&mut [f32]>] as *mut [&mut [f32]])
    };

    outputs
}

#[inline(always)]
fn slice_node_ports<'a>(
    buffer: &'a [f32],
    offset: usize,
    block_size: usize,
    chans: usize,
) -> &'a [&'a [f32]] {
    let end = (block_size * chans) + offset;

    let node_buffer = &buffer[offset..end];

    let slices = node_buffer.chunks_exact(block_size);

    assert_eq!(slices.len(), chans);

    let mut outputs_raw: [MaybeUninit<&[f32]>; MAX_ARITY] =
        unsafe { MaybeUninit::uninit().assume_init() };

    for (i, slice) in slices.enumerate() {
        outputs_raw[i] = MaybeUninit::new(slice);
    }

    // TODO: Evaluate safety!
    let outputs: &[&[f32]] =
        unsafe { &*(&outputs_raw[..chans] as *const [MaybeUninit<&[f32]>] as *const [&[f32]]) };

    outputs
}
