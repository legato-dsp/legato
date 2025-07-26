use crate::mini_graph::bang::Bang;
use crate::mini_graph::node::Node;

pub trait AudioGraph<const BUFFER_SIZE: usize, const CHANNEL_COUNT: usize> {
    fn next_block(&mut self) -> &Frame<BUFFER_SIZE, CHANNEL_COUNT>;
    fn invalidate_sort_order(&mut self);
}

use super::buffer::{Buffer, Frame};
use super::node::BoxedNode;
use super::graph::{DynamicGraph, Graph};

const MAXIMUM_BANG_INPUT_PORTS: usize = 4;

/// A resizable audio graph for experimentation. Pre-allocated, but not realtime safe, as the vector could grow.
/// We will soon add a fixed size, no_std graph for better real-time performance 
pub struct DynamicAudioGraph<const BUFFER_SIZE: usize, const CHANNEL_COUNT: usize> {
    graph: DynamicGraph<BoxedNode<BUFFER_SIZE, CHANNEL_COUNT>>,
    // Audio Work Buffers
    audio_inputs_buffer: Vec<Frame<BUFFER_SIZE, CHANNEL_COUNT>>, // A preallocated vector that contains a node's inputs
    audio_output_buffers: Vec<Frame<BUFFER_SIZE, CHANNEL_COUNT>>, // A preallocated vector that nodes write to
    // Bang Work Buffers
    bang_inputs_buffer: Vec<Bang>, // A preallocated vector that contains a node's inputs
    bang_output_buffers: Vec<Bang>,
    // Cached sort order that is invalidated when adding a new edge
    sort_order: Vec<usize>,
    // Index that our node delivers the final sample from
    sink_index: usize,
}

impl<const BUFFER_SIZE: usize, const CHANNEL_COUNT: usize> DynamicAudioGraph<BUFFER_SIZE, CHANNEL_COUNT> {
    pub fn with_capacity(capacity: usize) -> Self {
        let graph = DynamicGraph::with_capacity(capacity);
        Self {
            graph,
            audio_inputs_buffer: Vec::with_capacity(capacity),
            audio_output_buffers: vec![[Buffer::<BUFFER_SIZE>::default(); CHANNEL_COUNT]; capacity],
            bang_inputs_buffer: vec![Bang::Empty; MAXIMUM_BANG_INPUT_PORTS],
            bang_output_buffers: vec![Bang::Empty; capacity],
            sort_order: Vec::with_capacity(capacity),
            sink_index: 0,
        }
    }

    pub fn add_node(&mut self, node: BoxedNode<BUFFER_SIZE, CHANNEL_COUNT>) -> usize {
        let id = self.graph.add_node(node);
        self.invalidate_sort_order(); // Needed because you can add a node with no edges in theory
        id
    }

    pub fn add_edge(&mut self, from: usize, to: usize) {
        self.graph.add_edge(from, to);
        self.invalidate_sort_order();
    }

    pub fn add_edges(&mut self, edges: &[(usize, usize)]) {
        self.graph.add_edges(edges);
        self.invalidate_sort_order();
    }

    pub fn set_sink_index(&mut self, sink: usize) {
        self.sink_index = sink;
    }

    fn invalidate_sort_order(&mut self) {
        match self.graph.topo_sort() {
            Ok(order) => self.sort_order = order,
            Err(_) => panic!("Cycle detected in audio graph"),
        }
    }

    #[inline(always)]
    pub fn next_block(&mut self, inputs: Option<&[Frame<BUFFER_SIZE,CHANNEL_COUNT>]>) -> &Frame<BUFFER_SIZE, CHANNEL_COUNT> {
        for (i, &node_index) in self.sort_order.iter().enumerate() {
            let node = &mut self.graph.nodes[node_index];
            
            let incoming_nodes = &self.graph.incoming[node_index];

            self.bang_inputs_buffer.iter_mut().for_each(|x| {
                *x = Bang::Empty
            });

            for (i, &src) in incoming_nodes.iter().enumerate() {
                self.bang_inputs_buffer[i] = self.bang_output_buffers[src];
            }

            node.handle_bang(&self.bang_inputs_buffer.as_slice(), &mut self.bang_output_buffers[node_index]);

            // Here, we clear our preallocated buffer of inputs for a node
            // Then, we push all of the samples from each input that the current node is dependent on

            self.audio_inputs_buffer.clear();
            self.audio_inputs_buffer.reserve(incoming_nodes.len());

            for &src in incoming_nodes {
                self.audio_inputs_buffer.push(self.audio_output_buffers[src]);
            }

            // Also, if our node has inputs, and it is the first in the sort order, we use that instead
            // This models something like a reverb graph being used as a node, with an input from say live audio or a sample

            if i == 0 {
                if let Some(input) = inputs {
                    node.process(input, &mut self.audio_output_buffers[node_index]);
                    continue;
                }
            }
            node.process(&self.audio_inputs_buffer, &mut self.audio_output_buffers[node_index]);
        }

        &self.audio_output_buffers[self.sink_index]
    }
}


impl<const BUFFER_SIZE: usize, const CHANNEL_COUNT: usize> Node<BUFFER_SIZE, CHANNEL_COUNT> for DynamicAudioGraph<BUFFER_SIZE, CHANNEL_COUNT> {
    fn process(&mut self, inputs: &[Frame<BUFFER_SIZE, CHANNEL_COUNT>], output: &mut Frame<BUFFER_SIZE, CHANNEL_COUNT>) {
        let next_block = self.next_block(Some(inputs));
        for (output, input) in output.iter_mut().zip(next_block) {
            *output = *input;
        }
    }
}