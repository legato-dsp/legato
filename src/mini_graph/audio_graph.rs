use std::time::Duration;

use hashbrown::HashMap;
use indexmap::IndexSet;

use crate::mini_graph::bang::Bang;
use crate::mini_graph::node::{AudioNode, AudioUnit};
use crate::nodes::audio::adsr::ADSR;
use crate::nodes::audio::comb_filter::CombFilter;
use crate::nodes::audio::delay::{DelayLine, DelayTap, DelayWrite};
use crate::nodes::audio::filters::{FilterType, Svf};
use crate::nodes::audio::gain::Gain;
use crate::nodes::audio::hard_clipper::HardClipper;
use crate::nodes::audio::mixer::Mixer;
use crate::nodes::audio::osc::{Oscillator, Wave};
use crate::nodes::audio::sampler::Sampler;
use crate::nodes::control::clock::Clock;
use crate::nodes::control::iter::BangIter;
use crate::nodes::control::lfo::Lfo;
use crate::nodes::control::log::Log;
use crate::nodes::control::signal::{build_signal, Param};

pub trait AudioGraph<const BUFFER_SIZE: usize, const CHANNEL_COUNT: usize> {
    fn next_block(&mut self) -> &Frame<BUFFER_SIZE, CHANNEL_COUNT>;
    fn invalidate_sort_order(&mut self);
}

use super::buffer::{Buffer, Frame};
use super::graph::{DynamicGraph, Graph};

const MAXIMUM_BANG_INPUT_PORTS: usize = 4;

pub struct AudioContext<const BUFFER_SIZE: usize, const CHANNEL_COUNT: usize> {
    // Audio Work Buffers
    audio_inputs_buffer: Vec<Frame<BUFFER_SIZE, CHANNEL_COUNT>>, // A preallocated vector that contains a node's inputs. This is cleared and found for each node
    audio_output_buffers: Vec<Frame<BUFFER_SIZE, CHANNEL_COUNT>>, // A preallocated vector that nodes write to
    // Bang Work Buffers
    bang_inputs_buffer: Vec<Bang>, // A preallocated vector that contains a node's inputs
    bang_output_buffers: Vec<Bang>,
    // Delay Lines
    delay_lines: Vec<DelayLine<BUFFER_SIZE, CHANNEL_COUNT>>,
    delay_name_to_index_map: HashMap<&'static str, usize>
}
impl<const N: usize, const C: usize> AudioContext<N, C>{
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            audio_inputs_buffer: Vec::with_capacity(capacity),
            audio_output_buffers: vec![[Buffer::<N>::default(); C]; capacity],
            bang_inputs_buffer: vec![Bang::Empty; MAXIMUM_BANG_INPUT_PORTS],
            bang_output_buffers: vec![Bang::Empty; capacity],
            delay_lines: Vec::new(), // None of this is allocated yet
            delay_name_to_index_map: HashMap::new()
        }
    }
    #[inline(always)]
    fn execute_audio_unit(&mut self, audio_unit: &mut AudioUnit<N, C>, node_index: &usize, dependencies: &IndexSet<usize>, override_inputs: Option<&[Frame<N,C>]>){
        self.handle_bang_audio_unit(audio_unit, node_index, dependencies);
        self.process_audio_unit(audio_unit, node_index, dependencies, override_inputs);
    }
    #[inline(always)]
    fn handle_bang_audio_unit(&mut self, audio_unit: &mut AudioUnit<N, C>, node_index: &usize, dependencies: &IndexSet<usize>){
        self.bang_inputs_buffer.iter_mut().for_each(|x| {
            *x = Bang::Empty
        });

        for (i, &src) in dependencies.iter().enumerate() {
            self.bang_inputs_buffer[i] = self.bang_output_buffers[src];
        }

        match audio_unit {
            AudioUnit::AudioNode { node } => node.handle_bang(&self.bang_inputs_buffer.as_slice(), &mut self.bang_output_buffers[*node_index]),
            AudioUnit::DelayTapNode { node } => node.handle_bang(&self.bang_inputs_buffer.as_slice(), &mut self.bang_output_buffers[*node_index]),
            AudioUnit::DelayWriteNode { node } => node.handle_bang(&self.bang_inputs_buffer.as_slice(), &mut self.bang_output_buffers[*node_index]),
            AudioUnit::IONode { node } => node.handle_bang(&self.bang_inputs_buffer.as_slice(), &mut self.bang_output_buffers[*node_index]),
        }
    }
    #[inline(always)]
    fn process_audio_unit(&mut self, audio_unit: &mut AudioUnit<N, C>, node_index: &usize, dependencies: &IndexSet<usize>, override_inputs: Option<&[Frame<N,C>]>){
        match audio_unit {
            AudioUnit::AudioNode { node } => {
                self.audio_inputs_buffer.clear();
                self.audio_inputs_buffer.reserve(dependencies.len());

                for &src in dependencies {
                    self.audio_inputs_buffer.push(self.audio_output_buffers[src]);
                }

                let output = &mut self.audio_output_buffers[*node_index];

                if let Some(inputs) = override_inputs {
                    node.process(inputs, output);
                }
                else {
                    node.process(&self.audio_inputs_buffer, output);
                }
            },
            AudioUnit::DelayWriteNode { node } => {
                self.audio_inputs_buffer.clear();
                self.audio_inputs_buffer.reserve(dependencies.len());

                for &src in dependencies {
                    self.audio_inputs_buffer.push(self.audio_output_buffers[src]);
                }

                let delay_index = node.get_delay_line_index();

                let delay_line = self.delay_lines.get_mut(*delay_index).expect("Delay line out of bounds!");
        
                if let Some(inputs) = override_inputs {
                    node.process(inputs, delay_line);
                }
                else {
                    node.process(&self.audio_inputs_buffer, delay_line);
                }
            },
            AudioUnit::DelayTapNode { node } => {
                let delay_index = node.get_delay_line_index();
                if let Some(delay_buffer) = self.delay_lines.get_mut(*delay_index) {
                    let output = &mut self.audio_output_buffers[*node_index];

                    node.process(delay_buffer, output);
                }
            },
            AudioUnit::IONode { node } => {
                let output = &mut self.audio_output_buffers[*node_index];
                node.process(output);
            }
        }
    }
    #[inline(always)]
    fn get_buffer_at_index(&self, index: usize) -> &Frame<N, C> {
        &self.audio_output_buffers[index]
    }
    fn add_delay_line(&mut self, name: &'static str, capacity: usize) -> usize {
        let new_delay_line = DelayLine::new(capacity);
        let index = self.delay_lines.len();
        self.delay_lines.push(new_delay_line);
        self.delay_name_to_index_map.insert(name, index);
        index
    }
    fn get_delay_line_index(&self, name: &'static str) -> Option<usize> {
        self.delay_name_to_index_map.get(name).copied()
    }
}

/// A resizable audio graph for experimentation. Pre-allocated, but not realtime safe, as the underlying vectors can grow.
/// We will soon add a fixed size, no_std graph for better real-time performance, or more suitable for
///  sub-graph development.
pub struct DynamicAudioGraph<const N: usize, const C: usize> {
    graph: DynamicGraph<AudioUnit<N, C>>,
    audio_context: AudioContext<N, C>,
    // Cached sort order that is invalidated when adding a new edge
    sort_order: Vec<usize>,
    // Index that our node delivers the final sample from
    sink_index: usize,
}

impl<const N: usize, const C: usize> DynamicAudioGraph<N, C> {
    pub fn with_capacity(capacity: usize) -> Self {
        let graph = DynamicGraph::with_capacity(capacity);
        Self {
            graph,
            audio_context: AudioContext::with_capacity(capacity),
            sort_order: Vec::with_capacity(capacity),
            sink_index: 0,
        }
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
    pub fn next_block(&mut self, inputs: Option<&[Frame<N,C>]>) -> &Frame<N, C> {
        for (i, &node_index) in self.sort_order.iter().enumerate() {
            let audio_unit = &mut self.graph.nodes[node_index];
            
            let incoming_nodes = &self.graph.incoming[node_index];

            let override_inputs = if i == 0 { inputs } else { None };
            self.audio_context.execute_audio_unit(audio_unit, &node_index, incoming_nodes, override_inputs);
        }

        self.audio_context.get_buffer_at_index(self.sink_index)
    }
}

impl<const BUFFER_SIZE: usize, const CHANNEL_COUNT: usize> AudioNode<BUFFER_SIZE, CHANNEL_COUNT> for DynamicAudioGraph<BUFFER_SIZE, CHANNEL_COUNT> {
    fn process(&mut self, inputs: &[Frame<BUFFER_SIZE, CHANNEL_COUNT>], output: &mut Frame<BUFFER_SIZE, CHANNEL_COUNT>) {
        let next_block = self.next_block(Some(inputs));
        for (output, input) in output.iter_mut().zip(next_block) {
            *output = *input;
        }
    }
}

pub enum AddNodeProps<const N: usize, const C: usize> {
    // AudioNodes
    ADSR { sample_rate: u32} ,
    DelayWrite { delay_line_name: &'static str, capacity: usize, name: &'static str } ,
    DelayTap { gain: f32, sample_offset: f32, name: &'static str} ,
    Filter { sample_rate: f32, filter_type: FilterType, cutoff: f32, gain: f32, q: f32 },
    Gain { gain: f32 },
    HardClipper { limit: f32 },
    Mixer,
    Oscillator { freq: f32, sample_rate: u32, phase: f32, wave: Wave },
    CombFilter { delay_len: usize, feedback: f32 },
    // BangNodes
    Clock { sample_rate: u32, rate: Duration },
    Iter { values: &'static [Bang]},
    Lfo { freq: f32, offset: f32, amp: f32, phase: f32, sample_rate: f32 },
    Log,
    MidiToF,
    Graph { graph: DynamicAudioGraph<N, C> },
    Sampler { node: Sampler<N, C>}
}


pub trait AudioGraphApi<const N: usize, const C: usize> {
    fn add_audio_unit(&mut self, props: AddNodeProps<N, C>) -> usize;
    fn add_control_unit(&mut self, min: f32, max: f32, initial: f32) -> (usize, Param);
}

impl<const N: usize, const C: usize> AudioGraphApi<N, C> for DynamicAudioGraph<N, C> {
    // This probably won't scale well, perhaps a macro in the future?
    fn add_audio_unit(&mut self, props: AddNodeProps<N, C>) -> usize {
        let node: AudioUnit<N, C> = match props {
            AddNodeProps::ADSR { sample_rate } => AudioUnit::AudioNode {
                node: Box::new(ADSR::new(sample_rate)),
            },
            AddNodeProps::Filter { sample_rate, filter_type, cutoff, gain, q } => AudioUnit::AudioNode {
                node: Box::new(Svf::new(sample_rate, filter_type, cutoff, gain, q)),
            },
            AddNodeProps::Gain { gain } => AudioUnit::AudioNode {
                node: Box::new(Gain::new(gain)),
            },
            AddNodeProps::HardClipper { limit } => AudioUnit::AudioNode {
                node: Box::new(HardClipper::new(limit)),
            },
            AddNodeProps::Mixer => AudioUnit::AudioNode {
                node: Box::new(Mixer {}),
            },
            AddNodeProps::Oscillator { freq, sample_rate, phase, wave } => AudioUnit::AudioNode {
                node: Box::new(Oscillator::new(freq, sample_rate, phase, wave)),
            },
            AddNodeProps::CombFilter { delay_len, feedback } => AudioUnit::AudioNode {
                node: Box::new(CombFilter::new(delay_len, feedback)),
            },
            // Bang Nodes
            AddNodeProps::Clock { sample_rate, rate } => AudioUnit::AudioNode {
                node: Box::new(Clock::new(sample_rate, rate)),
            },
            AddNodeProps::Iter { values } => AudioUnit::AudioNode {
                node: Box::new(BangIter::new(values)),
            },
            AddNodeProps::Lfo { freq, offset, amp, phase, sample_rate } => AudioUnit::AudioNode {
                node: Box::new(Lfo::new(freq, offset, amp, phase, sample_rate)),
            },
            AddNodeProps::Log => AudioUnit::AudioNode {
                node: Box::new(Log {}),
            },
            AddNodeProps::MidiToF => AudioUnit::AudioNode {
                node: Box::new(Log {}),
            },
            // Delay Nodes
            AddNodeProps::DelayWrite { delay_line_name, capacity, name } => {
                let index = self.audio_context.add_delay_line(delay_line_name, capacity);
                let new_node = DelayWrite::new(name, index);
                
                AudioUnit::DelayWriteNode { node: Box::new(new_node) }
            },
            AddNodeProps::DelayTap { name, gain, sample_offset } => {
                let index = self.audio_context.get_delay_line_index(name).expect("Delay line not found!");
                let new_node = DelayTap::new(sample_offset, index, gain);
                
                AudioUnit::DelayTapNode { node: Box::new(new_node) }
            },
            AddNodeProps::Graph { graph } => AudioUnit::AudioNode {
                node: Box::new(graph),
            },
            AddNodeProps::Sampler { node } => AudioUnit::IONode { node: Box::new(node) }
        };
        let index = self.graph.add_node(node);
        self.invalidate_sort_order();
        index
    }
    // Slightly different signature here, although the Signal is technically a node.
    fn add_control_unit(&mut self, min: f32, max: f32, initial: f32) -> (usize, Param) {
        let (node, param ) = build_signal(min, max, initial);
        let index = self.graph.add_node(AudioUnit::AudioNode { node: Box::new(node) });
        self.invalidate_sort_order();
        (index, param)
    }
}