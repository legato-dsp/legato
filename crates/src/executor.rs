use crate::{
    context::AudioContext,
    graph::{AudioGraph, GraphError},
    node::Inputs,
    runtime::NodeKey,
};
use slotmap::SecondaryMap;

pub const MAX_ARITY: usize = 32;

/// For the time being, we just check if it has been prepared or not,
/// but in the future we might pause, stop, etc.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum ExecutorState {
    Prepared,
    #[default]
    Unprepared,
}

/// We use this struct to easily slice in other contexts,
/// and we can slice later with this owned array.
///
/// Otherwise,
pub struct OutputView<'a> {
    pub channels: [&'a [f32]; MAX_ARITY],
    pub chans: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Executor {
    data: Box<[f32]>,
    scratch: Box<[f32]>,
    pub graph: AudioGraph,
    node_offsets: SecondaryMap<NodeKey, usize>,
    // Keys for inputs/output nodes
    source_key: Option<NodeKey>,
    sink_key: Option<NodeKey>,
    state: ExecutorState,
}

impl Executor {
    /// Set the sink key for the runtime
    pub fn set_sink(&mut self, key: NodeKey) -> Result<(), GraphError> {
        match self.graph.exists(key) {
            true => {
                self.sink_key = Some(key);
                Ok(())
            }
            false => Err(GraphError::NodeDoesNotExist),
        }
    }

    /// Set the source key for the runtime
    pub fn set_source(&mut self, key: NodeKey) -> Result<(), GraphError> {
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
    pub fn prepare(&mut self, block_size: usize) {
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
    pub fn process(
        &mut self,
        ctx: &mut AudioContext,
        external_inputs: Option<&Inputs>,
    ) -> OutputView<'_> {
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

            // Check and see if we have external inputs
            let valid_external_inputs = self.source_key.is_some()
                && self.source_key.unwrap() == *node_key
                && external_inputs.as_ref().is_some();

            if valid_external_inputs {
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

                    let scratch_start = conn.sink.port_index * block_size;
                    let scratch_end = scratch_start + block_size;

                    self.scratch[scratch_start..scratch_end]
                        .iter_mut()
                        .zip(buffer.iter())
                        .for_each(|(dst, src)| *dst += src);
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

            let mut active_outputs =
                slice_node_ports_mut(&mut self.data, node_start, block_size, audio_outputs_size);

            node.process(
                ctx,
                &inputs[0..audio_inputs_size],
                &mut active_outputs[0..audio_outputs_size],
            );
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

        OutputView {
            channels: final_outputs,
            chans: node_arity,
        }
    }
}

#[inline(always)]
fn slice_node_ports(
    buffer: &[f32],
    offset: usize,
    block_size: usize,
    chans: usize,
) -> [&[f32]; MAX_ARITY] {
    let node_end = (block_size * chans) + offset;
    let node_buffer = &buffer[offset..node_end];

    let mut chunks = node_buffer.chunks_exact(block_size);

    std::array::from_fn(|_| chunks.next().unwrap_or_default())
}

#[inline(always)]
fn slice_node_ports_mut(
    buffer: &mut [f32],
    offset: usize,
    block_size: usize,
    chans: usize,
) -> [&mut [f32]; MAX_ARITY] {
    let node_end = (block_size * chans) + offset;
    let node_buffer = &mut buffer[offset..node_end];

    let chunks = &mut node_buffer.chunks_exact_mut(block_size);

    std::array::from_fn(|_| chunks.next().unwrap_or_default())
}
