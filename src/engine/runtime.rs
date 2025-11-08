use crate::engine::{
    audio_context::AudioContext,
    buffer::{Buffer, Frame},
    graph::{AudioGraph, AudioNode, Connection, GraphError, NodeKey},
    node::Node,
    port::{PortRate, PortedErased, Ports},
};
use generic_array::ArrayLength;
use slotmap::SecondaryMap;
use typenum::U0;

// Arbitrary max init. inputs
pub const MAX_INITIAL_INPUTS: usize = 32;

pub struct Runtime<const AF: usize, const CF: usize, C, Ci>
where
    C: ArrayLength,
    Ci: ArrayLength,
{
    // Audio context containing sample rate, control rate, etc.
    context: AudioContext<AF>,
    graph: AudioGraph<AF, CF>,
    // Where the nodes write their output to, so node sinks / port sources
    port_sources_audio: SecondaryMap<NodeKey, Vec<Buffer<AF>>>,
    port_sources_control: SecondaryMap<NodeKey, Vec<Buffer<CF>>>,
    // Preallocated buffers for delivering samples
    audio_inputs_scratch_buffers: Vec<Buffer<AF>>,
    control_inputs_scratch_buffers: Vec<Buffer<CF>>,
    // A sink key for pulling the final processed buffer. Optional for graph construction, but required at runtime
    sink_key: Option<NodeKey>,
    ports: Ports<C, C, Ci, U0>, // Here, we assume that audio in and out is the same arity. If you need something different, pair with a mixer
}
impl<'a, const AF: usize, const CF: usize, C, Ci> Runtime<AF, CF, C, Ci>
where
    C: ArrayLength,
    Ci: ArrayLength,
{
    pub fn new(
        context: AudioContext<AF>,
        graph: AudioGraph<AF, CF>,
        ports: Ports<C, C, Ci, U0>,
    ) -> Self {
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
            ports,
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
    pub fn get_context_mut(&mut self) -> &mut AudioContext<AF> {
        &mut self.context
    }
    // F32 is a bit weird here, but we cast so frequently why not
    pub fn get_sample_rate(&self) -> f32 {
        self.context.get_sample_rate()
    }
    // TODO: Graphs as nodes again
    pub fn next_block(
        &mut self,
        external_inputs: Option<(&Frame<AF>, &Frame<CF>)>,
    ) -> &[Buffer<AF>] {
        let (sorted_order, nodes, incoming) = self.graph.get_sort_order_nodes_and_runtime_info(); // TODO: I don't like this, feels like incorrect ownership

        for (i, node_key) in sorted_order.iter().enumerate() {
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

            // Pass in inputs if they exist to source node. In the future, maybe make this explicity rather than from topo sort
            if i == 0 && external_inputs.is_some() {
                let (ai, ci) = external_inputs.unwrap();
                for c in 0..C::USIZE {
                    self.audio_inputs_scratch_buffers[c].copy_from_slice(&ai[c]);
                    self.control_inputs_scratch_buffers[c].copy_from_slice(&ci[c]);
                }
            } else {
                let incoming = incoming.get(*node_key).expect("Invalid connection!");

                for connection in incoming {
                    // Write all incoming data from the connection and port, to the current node, and the sink port
                    debug_assert!(connection.sink.node_key == *node_key);
                    match (connection.source.port_rate, connection.sink.port_rate) {
                        (PortRate::Audio, PortRate::Audio) => {
                            for n in 0..AF {
                                self.audio_inputs_scratch_buffers[connection.sink.port_index][n] +=
                                    self.port_sources_audio[connection.source.node_key]
                                        [connection.source.port_index][n];
                            }
                        }
                        (PortRate::Control, PortRate::Control) => {
                            for n in 0..AF {
                                self.control_inputs_scratch_buffers[connection.sink.port_index]
                                    [n] += self.port_sources_control[connection.source.node_key]
                                    [connection.source.port_index][n];
                            }
                        }
                        (PortRate::Audio, PortRate::Control) => {
                            panic!("Audio to control not currently supported")
                        }
                        (PortRate::Control, PortRate::Audio) => {
                            todo!("Control to audio not currently supported")
                        }
                    };
                }
            }

            let audio_output_buffer = &mut self.port_sources_audio[*node_key];
            let control_output_buffer = &mut self.port_sources_control[*node_key];

            let node = nodes
                .get_mut(*node_key)
                .expect("Could not find node at index {node_index:?}");

            node.process(
                &mut self.context,
                &self.audio_inputs_scratch_buffers[0..audio_input_size],
                audio_output_buffer.as_mut_slice(),
                &self.control_inputs_scratch_buffers[0..audio_input_size],
                control_output_buffer.as_mut_slice(),
            );
        }

        let sink_key = self.sink_key.expect("Sink node must be provided");
        self.port_sources_audio
            .get(sink_key)
            .expect("Invalid output port!")
            .as_slice()
    }
}

impl<const AF: usize, const CF: usize, C, Ci> Node<AF, CF> for Runtime<AF, CF, C, Ci>
where
    C: ArrayLength,
    Ci: ArrayLength,
{
    fn process(
        &mut self,
        _: &mut AudioContext<AF>,
        ai: &Frame<AF>,
        ao: &mut Frame<AF>,
        ci: &Frame<CF>,
        _: &mut Frame<CF>, // Subgraphs not forwarding control at the moment
    ) {
        let outputs = self.next_block(Some((ai, ci)));
        for c in 0..C::USIZE {
            ao[c].copy_from_slice(&outputs[c]);
        }
    }
}

impl<const AF: usize, const CF: usize, C, Ci> PortedErased for Runtime<AF, CF, C, Ci>
where
    C: ArrayLength,
    Ci: ArrayLength,
{
    fn get_audio_inputs(&self) -> Option<&[super::port::AudioInputPort]> {
        self.ports.get_audio_inputs()
    }
    fn get_audio_outputs(&self) -> Option<&[super::port::AudioOutputPort]> {
        self.ports.get_audio_outputs()
    }
    fn get_control_inputs(&self) -> Option<&[super::port::ControlInputPort]> {
        self.ports.get_control_inputs()
    }
    fn get_control_outputs(&self) -> Option<&[super::port::ControlOutputPort]> {
        self.ports.get_control_outputs()
    }
}

pub fn build_runtime<const AF: usize, const CF: usize, C, Ci>(
    initial_capacity: usize,
    sample_rate: f32,
    control_rate: f32,
    ports: Ports<C, C, Ci, U0>,
) -> Runtime<AF, CF, C, Ci>
where
    C: ArrayLength,
    Ci: ArrayLength,
{
    let graph = AudioGraph::with_capacity(initial_capacity);
    let context = AudioContext::new(sample_rate, control_rate);

    Runtime::<AF, CF, C, Ci>::new(context, graph, ports)
}

/// This trait allows us to erase runtime generics,
/// for to more easily add oversampled subgraphs to
/// an existing runtime.
pub trait RuntimeErased<const AF: usize, const CF: usize>: Node<AF, CF> {
    fn next_block(&mut self, external_inputs: Option<(&Frame<AF>, &Frame<CF>)>) -> &[Buffer<AF>];
}

impl<const AF: usize, const CF: usize, C, Ci> RuntimeErased<AF, CF> for Runtime<AF, CF, C, Ci>
where
    C: ArrayLength,
    Ci: ArrayLength,
{
    fn next_block(&mut self, external_inputs: Option<(&Frame<AF>, &Frame<CF>)>) -> &[Buffer<AF>] {
        self.next_block(external_inputs)
    }
}
