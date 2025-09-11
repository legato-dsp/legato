use slotmap::SecondaryMap;

use crate::engine::{
    audio_context::AudioContext,
    buffer::Buffer,
    graph::{AudioGraph, AudioNode, Connection, GraphError, NodeKey},
    port::{DownsampleAlg, PortRate, UpsampleAlg}, utils::{downsample_first_sample, upsample_linear, upsample_zoh},
};

// Arbitrary max init. inputs
const MAX_INITIAL_INPUTS: usize = 8;

pub struct Runtime<const AF: usize, const CF: usize, const CHANNELS: usize> {
    // Audio context containing sample rate, control rate, etc.
    context: AudioContext,
    graph: AudioGraph<AF, CF>,
    // Where the nodes write their output to, so node sinks / port sources
    port_sources_audio: SecondaryMap<NodeKey, Vec<Buffer<AF>>>,
    port_sources_control: SecondaryMap<NodeKey, Vec<Buffer<CF>>>, 
    // Preallocated buffers for delivering samples
    audio_inputs_scratch_buffers: Vec<Buffer<AF>>,
    control_inputs_scratch_buffers: Vec<Buffer<CF>>,
    // An optional sink key for chaining graphs as nodes, delivering f32 values, etc. 
    sink_key: Option<NodeKey>,
}
impl<'a, const AF: usize, const CF: usize, const CHANNELS: usize> Runtime<AF, CF, CHANNELS> {
    pub fn new(context: AudioContext, graph: AudioGraph<AF, CF>) -> Self {
        let audio_sources = SecondaryMap::with_capacity(graph.len());
        let control_sources = SecondaryMap::with_capacity(graph.len());
        Self {
            context,
            graph,
            port_sources_audio: audio_sources,
            port_sources_control: control_sources,
            audio_inputs_scratch_buffers: vec![Buffer::<AF>::SILENT; MAX_INITIAL_INPUTS],
            control_inputs_scratch_buffers: vec![Buffer::<CF>::SILENT; MAX_INITIAL_INPUTS],
            sink_key: None,
        }
    }
    pub fn add_node(&mut self, node: AudioNode<AF, CF>) -> NodeKey {
        let audio_inputs_length = node.get_audio_outputs().map_or(0, |f| f.len());
        let control_inputs_length = node.get_control_outputs().map_or(0, |f| f.len());

        let node_key = self.graph.add_node(node);

        self.port_sources_audio
            .insert(node_key, vec![Buffer::<AF>::SILENT; audio_inputs_length]);
        self.port_sources_control
            .insert(node_key, vec![Buffer::<CF>::SILENT; control_inputs_length]);

        node_key
    }
    pub fn remove_node(&mut self, key: NodeKey) {
        self.graph.remove_node(key);
        self.port_sources_audio.remove(key);
    }
    pub fn add_edge(&mut self, connection: Connection) -> Result<Connection, GraphError> {
        self.graph.add_edge(connection)
    }
    pub fn remove_edge(&mut self, connection: Connection) -> Result<(), GraphError> {
        self.graph.remove_edge(connection)
    }
    pub fn set_sink_key(&mut self, key: NodeKey) -> Result<(), GraphError> {
        match self.graph.exists(key) {
            true => {
                self.sink_key = Some(key);
                Ok(())
            }
            false => Err(GraphError::NodeDoesNotExist),
        }
    }
    // TODO: Graphs as nodes again
    pub fn next_block(&mut self) -> &[Buffer<AF>] {
        let (sorted_order, nodes, incoming) = self.graph.get_nodes_and_runtime_info(); // TODO: I don't like this
        for node_key in sorted_order.iter() {
            // Reset all of the inputs about to be passed into this node
            let audio_input_size = nodes[*node_key].get_audio_inputs().map_or(0, |f| f.len());
            let control_input_size = nodes[*node_key].get_control_inputs().map_or(0, |f| f.len());

            // Zero the incoming buffers
            self.audio_inputs_scratch_buffers[..audio_input_size]
                .iter_mut()
                .for_each(|buf| buf.fill(0.0));

            self.control_inputs_scratch_buffers[..control_input_size]
                .iter_mut()
                .for_each(|buf| buf.fill(0.0));

            let incoming = incoming.get(*node_key).expect("Invalid connection!");

            for connection in incoming {
                // Write all incoming data from the connection and port, to the current node, and the sink port
                debug_assert!(connection.sink.node_key == *node_key);
                // For the time being, we panic here, as this shows that something is wrong with our graph construction logic
                let sink_node = nodes.get(connection.sink.node_key).unwrap_or_else(|| panic!("Invalid sink node! {:?}", connection));

                match (connection.source.port_rate, connection.sink.port_rate) {
                    (PortRate::Audio, PortRate::Control) => {
                        // downsample audio to control, using spec on the port
                        let audio_out = &self.port_sources_audio[connection.source.node_key][connection.source.port_index];
                        let control_inputs = &mut self.control_inputs_scratch_buffers[connection.sink.port_index];

                        let sink_port = &sink_node.get_control_inputs().expect("Could not find audio out")[connection.sink.port_index];

                        match sink_port.resample {
                            DownsampleAlg::FirstSample => downsample_first_sample(audio_out, control_inputs),
                        }
                    },
                    (PortRate::Control, PortRate::Audio) => {
                        // upsample control to audio, using spec on the port
                        let control_out = &self.port_sources_control[connection.source.node_key][connection.source.port_index];
                        let audio_inputs = &mut self.audio_inputs_scratch_buffers[connection.sink.port_index];

                        let sink_port = &sink_node.get_audio_inputs().expect("Could not find audio sink")[connection.sink.port_index];

                        match sink_port.resample {
                            UpsampleAlg::Lerp => upsample_linear(&control_out, audio_inputs),
                            UpsampleAlg::ZOH => upsample_zoh(&control_out, audio_inputs),
                        } 
                    },
                    (PortRate::Audio, PortRate::Audio) => {
                        self.audio_inputs_scratch_buffers[connection.sink.port_index] =
                            self.port_sources_audio[connection.source.node_key][connection.source.port_index];
                    },
                    (PortRate::Control, PortRate::Control) => {
                        self.control_inputs_scratch_buffers[connection.sink.port_index] =
                            self.port_sources_control[connection.source.node_key][connection.source.port_index];
                    }
                };
            }

            let audio_output_buffer = &mut self.port_sources_audio[*node_key]; // Let the node write to the output as a mut_slice
            let control_output_buffer = &mut self.port_sources_control[*node_key];

            let node = nodes
                .get_mut(*node_key)
                .expect("Could not find node at index {node_index:?}");

            node.process(
                &self.context,
                &self.audio_inputs_scratch_buffers[0..audio_input_size],
                audio_output_buffer.as_mut_slice(),
                &self.control_inputs_scratch_buffers[0..audio_input_size],
                control_output_buffer.as_mut_slice()
            );
        }

        let sink_key = self.sink_key.expect("Sink node must be provided");
        self.port_sources_audio
            .get(sink_key)
            .expect("Invalid output port!")
            .as_slice()
    }
}

pub fn build_runtime<const AF: usize, const CF: usize, const CHANNEL_SIZE: usize>(
    initial_capacity: usize,
    sample_rate: f32,
    control_rate: f32,
) -> Runtime<AF, CF, CHANNEL_SIZE> {
    let graph = AudioGraph::with_capacity(initial_capacity);
    let context = AudioContext::new(sample_rate, control_rate);

    Runtime::new(context, graph)
}
