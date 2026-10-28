use crate::{
    builder::ValidationError,
    context::AudioContext,
    graph::{AudioGraph, GraphError},
    node_state::{NodeRuntime, NodeState, Plan},
    ports::{Ports, Unpatched},
    runtime::NodeKey,
};
use slotmap::SecondaryMap;

pub const MAX_ARITY: usize = 32;

/// (input, output) port index pairs.
type BypassPairs = Box<[(usize, usize)]>;

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
pub struct OutputView<'a> {
    pub channels: [&'a [f32]; MAX_ARITY],
    pub chans: usize,
}

#[derive(Debug, Default)]
pub struct Executor {
    data: Box<[f32]>,
    scratch: Box<[f32]>,
    pub graph: AudioGraph,
    node_offsets: SecondaryMap<NodeKey, usize>,
    sink_key: Option<NodeKey>,
    state: ExecutorState,
    node_states: SecondaryMap<NodeKey, NodeRuntime>,
    /// (input, output) port pairs a bypassed node copies; `None` if it can't be bypassed.
    bypass_pairs: SecondaryMap<NodeKey, Option<BypassPairs>>,
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

    pub fn sink(&self) -> &Option<NodeKey> {
        &self.sink_key
    }

    /// Reject nodes wider than [`MAX_ARITY`].
    pub fn validate_arity(&self) -> Result<(), ValidationError> {
        for node in self.graph.nodes() {
            let ports = node.get_node().ports();

            for (count, dir) in [
                (ports.ports_in.len(), "input"),
                (ports.ports_out.len(), "output"),
            ] {
                if count > MAX_ARITY {
                    return Err(ValidationError::ArityExceeded(format!(
                        "node '{}' ({}) has {} audio {} ports, but the executor supports \
                         at most {} per side",
                        node.name, node.node_kind, count, dir, MAX_ARITY
                    )));
                }
            }
        }

        Ok(())
    }

    /// Reject graphs that leave a [`Unpatched::Required`] input unconnected.
    pub fn validate_required_inputs(&self) -> Result<(), ValidationError> {
        for (key, node) in self.graph.iter() {
            let connected: Vec<usize> = self
                .graph
                .incoming_connections(key)
                .into_iter()
                .flatten()
                .map(|c| c.sink.port_index)
                .collect();

            for port in &node.get_node().ports().ports_in {
                if port.unpatched == Unpatched::Required && !connected.contains(&port.index) {
                    return Err(ValidationError::UnconnectedInput(format!(
                        "node '{}' ({}) requires its '{}' input (index {}) to be connected",
                        node.name, node.node_kind, port.name, port.index
                    )));
                }
            }
        }

        Ok(())
    }

    /// Prepare the flat buffer allocation for the graph, as well as the node offsets.
    ///
    /// NOTE: This is not realtime safe!
    pub fn prepare(&mut self, block_size: usize, fade_len: u32) {
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
                .ports_out
                .len();

            total_ports += arity;
        }

        self.node_states.clear();
        self.bypass_pairs.clear();

        for (key, node) in self.graph.iter() {
            self.node_states.insert(key, NodeRuntime::new(fade_len));
            self.bypass_pairs
                .insert(key, bypass_pairs(node.get_node().ports()));
        }

        self.state = ExecutorState::Prepared;
    }

    /// Whether `key` has matching default inputs and outputs to bypass.
    ///
    /// This is because we don't want to say bypass three total inputs to a stereo output,
    /// we want developers to mark their default_in ports so we know what we can treat as
    /// bypassable audio. This also matches what various VST hosts expect.
    pub fn can_bypass(&self, key: NodeKey) -> bool {
        matches!(self.bypass_pairs.get(key), Some(Some(_)))
    }

    /// Fade `key` toward `state` from the next block. Returns false, changing
    /// nothing, for an unknown node or a bypass it can't do.
    pub fn set_state(&mut self, key: NodeKey, state: NodeState) -> bool {
        if state == NodeState::Bypassed && !self.can_bypass(key) {
            return false;
        }
        match self.node_states.get_mut(key) {
            Some(rt) if rt.target() != state => {
                rt.set_target(state);
                true
            }
            Some(_) => true,
            None => false,
        }
    }

    #[inline(always)]
    pub fn process(&mut self, ctx: &mut AudioContext) -> OutputView<'_> {
        assert!(self.state == ExecutorState::Prepared);

        let block_size = ctx.get_config().block_size;

        let (sorted_order, nodes, incoming) = self.graph.get_sort_order_nodes_and_runtime_info(); // TODO: I don't like this, feels like incorrect ownership

        for node_key in sorted_order {
            let ports = nodes[*node_key].get_node().ports();

            let audio_inputs_size = ports.ports_in.len();
            let audio_outputs_size = ports.ports_out.len();

            let node_start = *self.node_offsets.get(*node_key).unwrap();
            let node_end = node_start + audio_outputs_size * block_size;

            let plan = self.node_states[*node_key].plan();

            // If the node is suspended, simply zero the buffer
            if let Plan::Suspended { zero } = plan {
                if zero {
                    self.data[node_start..node_end].fill(0.0);
                }
                continue;
            }

            // Fill the scratch buffer for the first N channels
            self.scratch[..audio_inputs_size * block_size].fill(0.0);

            let mut inputs: [Option<&[f32]>; MAX_ARITY] = [None; MAX_ARITY];

            let mut has_inputs: [bool; MAX_ARITY] = [false; MAX_ARITY];

            let incoming = incoming
                .get(*node_key)
                .expect("Invalid connection in executor!");

            // Note(Luke): you'll notice that we may be able to skip a copy/addition here if we only have a single
            // input to a single channel.
            //
            // For instance, if an audio node has only one incoming stream per channel, this can directly
            // referenced instead of added/copied.
            //
            // However, this did not really show any provable gains, as the DSP math dwarfs this cost.
            // I'm open to other solutions here, but I just wanted to provide more context.
            //
            // So we do the general case here, which is adding the incoming block/stream to the block
            // that is being written to.
            for conn in incoming {
                // We're panicking here if we can't find the offset, as something would have gone wildly wrong...
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

            for i in 0..audio_inputs_size {
                if has_inputs[i] || ports.ports_in[i].unpatched == Unpatched::Silent {
                    // Pass references to each slice to the inputs now
                    let start = i * block_size;
                    let end = start + block_size;
                    inputs[i] = Some(&self.scratch[start..end]);
                }
            }

            let pairs = self.bypass_pairs[*node_key].as_deref().unwrap_or_default();
            let dry_in = |i: usize| &self.scratch[i * block_size..(i + 1) * block_size];

            if plan == Plan::Bypassed {
                // Zero this out first as we have no unified zero-ing out step
                self.data[node_start..node_end].fill(0.0);
                for &(i, o) in pairs {
                    let out = node_start + o * block_size;
                    self.data[out..out + block_size].copy_from_slice(dry_in(i));
                }
                continue;
            }

            let node = nodes
                .get_mut(*node_key)
                .expect("Could not find node at index {node_index:?}")
                .get_node_mut();

            let mut active_outputs =
                slice_node_ports_mut(&mut self.data, node_start, block_size, audio_outputs_size);

            node.process(
                ctx,
                &inputs[0..audio_inputs_size],
                &mut active_outputs[0..audio_outputs_size],
            );

            // We post process here given the state of the node.
            //
            // We do the following with these states:
            //
            // - Muted -> We still process and update internal state, but simply fill with 0 values.
            //
            // - Fading -> We post process and add a fade based on the internal state, we fade this against
            // the incoming pairs, which are other default_in/out pairs. For instance, a distortion might have
            // two default in ports, and one modulation port. We would not want to bypass the modulation as well.
            //
            // - Others -> The others were already silenced or bypassed, and don't require postprocessing
            match plan {
                Plan::Muted => self.data[node_start..node_end].fill(0.0),
                Plan::Fading => {
                    // Each channel replays the curve from the block's start.
                    let rt = &mut self.node_states[*node_key];

                    for chan in self.data[node_start..node_end].chunks_exact_mut(block_size) {
                        chan.iter_mut()
                            .zip(rt.gains())
                            .for_each(|(y, (wet, _))| *y *= wet);
                    }

                    for &(i, o) in pairs {
                        let out = node_start + o * block_size;
                        self.data[out..out + block_size]
                            .iter_mut()
                            .zip(dry_in(i))
                            .zip(rt.gains())
                            .for_each(|((sample_out, gain), (_, dry))| *sample_out += gain * dry);
                    }
                    rt.advance(block_size);
                }
                _ => {}
            }
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
            .ports_out
            .len();

        let final_outputs = slice_node_ports(&self.data, *node_offset, block_size, node_arity);

        OutputView {
            channels: final_outputs,
            chans: node_arity,
        }
    }
}

/// Default inputs paired with default outputs by position, if the counts match.
fn bypass_pairs(ports: &Ports) -> Option<BypassPairs> {
    let (ins, outs) = (ports.default_in(), ports.default_out());
    (ins.len() == outs.len()).then(|| ins.into_iter().zip(outs).collect())
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
