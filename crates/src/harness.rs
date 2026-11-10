//! Various harnesses for testing and benchmarking

use std::sync::Arc;

use slotmap::SlotMap;

use crate::{
    config::Config,
    context::AudioContext,
    graph::{Connection, ConnectionEntry},
    midi::{MidiEvent, MidiMessage, MidiMessageKind},
    midi_bus::NodeMidi,
    node::{DynNode, Inputs, LegatoNode, Node},
    ports::{PortBuilder, Ports, Signal},
    resources::{Resources, arena::RuntimeArena, params::ParamStore},
    runtime::Runtime,
};

/// We need a placeholder context for testing, this just mockes a few
/// [`rtrb::RingBuffer`]s needed for construction. This is a bit of
/// a smell but it is rleatively convenient for now.
pub fn build_placeholder_context(config: Config) -> AudioContext {
    let (_, dummy_sample_cons) = rtrb::RingBuffer::new(64);
    let (dummy_garbage_prod, _) = rtrb::RingBuffer::new(64);

    AudioContext::new(
        config,
        Resources::new(
            RuntimeArena::default(),
            ParamStore::new(Arc::new([])),
            dummy_sample_cons,
            dummy_garbage_prod,
            SlotMap::default(),
            SlotMap::default(),
            SlotMap::default(),
            SlotMap::default(),
        ),
    )
}

fn build_placeholder_runtime(config: Config) -> Runtime {
    let temporary_context = build_placeholder_context(config);

    Runtime::new(temporary_context)
}

pub fn get_node_test_harness_stereo_4096(node: Box<dyn DynNode>) -> Runtime {
    let config = Config {
        sample_rate: 48_000,
        block_size: 4096,
        channels: 2,
        rt_capacity: 0,
    };

    let mut runtime = build_placeholder_runtime(config);

    let id = runtime.add_node(LegatoNode::new("test node".into(), "test".into(), node));

    let _ = runtime.set_sink_key(id);

    runtime.prepare();

    runtime
}

pub fn get_node_test_harness_stereo(
    node: Box<dyn DynNode>,
    sr: usize,
    block_size: usize,
) -> Runtime {
    let config = Config {
        sample_rate: sr,
        block_size,
        channels: 2,
        rt_capacity: 0,
    };

    let mut runtime = build_placeholder_runtime(config);

    let id = runtime.add_node(LegatoNode::new("test node".into(), "test".into(), node));

    let _ = runtime.set_sink_key(id);

    runtime.prepare();

    runtime
}

/// An event at an absolute frame since the start of a [`MidiHarness`] run.
pub type TimedMidi = (u64, MidiMessage);

/// Runs one node over many blocks with scripted MIDI and stream inputs,
/// and collects what it writes to its first MIDI output.
///
/// Frames are absolute, so the same script and node give the same result at
/// any block size.
pub struct MidiHarness {
    sample_rate: usize,
    block_size: usize,
    midi: Vec<TimedMidi>,
    streams: Vec<(usize, SignalFn)>,
}

type SignalFn = Box<dyn FnMut(u64) -> f32 + Send>;

impl MidiHarness {
    pub fn new(sample_rate: usize, block_size: usize) -> Self {
        Self {
            sample_rate,
            block_size,
            midi: Vec::new(),
            streams: Vec::new(),
        }
    }

    /// Events for MIDI input 0; leaving this out leaves the input unpatched.
    pub fn midi_in(mut self, events: impl IntoIterator<Item = TimedMidi>) -> Self {
        self.midi.extend(events);
        self.midi.sort_by_key(|(frame, _)| *frame);
        self
    }

    /// A signal for stream input `port`, as a function of the absolute frame.
    pub fn stream_in(
        mut self,
        port: usize,
        signal: impl FnMut(u64) -> f32 + Send + 'static,
    ) -> Self {
        self.streams.push((port, Box::new(signal)));
        self
    }

    pub fn run(self, node: Box<dyn DynNode>, frames: usize) -> Vec<TimedMidi> {
        let config = Config {
            sample_rate: self.sample_rate,
            block_size: self.block_size,
            channels: 2,
            rt_capacity: 0,
        };
        let mut runtime = build_placeholder_runtime(config);

        let has_midi_out = !node.ports().midi_out.is_empty();
        let key = runtime.add_node(LegatoNode::new("under test".into(), "test".into(), node));
        let connect = |runtime: &mut Runtime, source, port_index, signal| {
            runtime
                .add_edge(Connection {
                    source: ConnectionEntry {
                        node_key: source,
                        port_index: 0,
                    },
                    sink: ConnectionEntry {
                        node_key: key,
                        port_index,
                    },
                    signal,
                })
                .expect("the node under test should have this input");
        };

        if !self.midi.is_empty() {
            let script = runtime.add_node(LegatoNode::new(
                "script".into(),
                "script".into(),
                Box::new(Script::new(self.midi)),
            ));
            connect(&mut runtime, script, 0, Signal::Midi);
        }
        for (port, signal) in self.streams {
            let source = runtime.add_node(LegatoNode::new(
                "signal".into(),
                "signal".into(),
                Box::new(StreamSource::new(signal)),
            ));
            connect(&mut runtime, source, port, Signal::Stream);
        }

        runtime.set_sink_key(key).unwrap();
        runtime.prepare();

        let mut out = Vec::new();
        let blocks = frames.div_ceil(self.block_size);
        for block in 0..blocks {
            runtime.next_block();
            if !has_midi_out {
                continue;
            }
            let start = (block * self.block_size) as u64;
            for event in runtime.midi_output(key, 0).unwrap_or_default() {
                out.push((start + event.frame as u64, event.msg));
            }
        }
        out
    }
}

/// Writes scripted events into the block they fall in.
struct Script {
    events: Vec<TimedMidi>,
    next: usize,
    block_start: u64,
    ports: Ports,
}

impl Script {
    fn new(events: Vec<TimedMidi>) -> Self {
        Self {
            events,
            next: 0,
            block_start: 0,
            ports: PortBuilder::default().midi_out_named(&["out"]).build(),
        }
    }
}

impl Node for Script {
    fn process(&mut self, ctx: &mut AudioContext, _: &Inputs, _: &mut [&mut [f32]]) {
        let block_size = ctx.get_config().block_size as u64;
        let end = self.block_start + block_size;
        let NodeMidi { mut outputs, .. } = ctx.node_midi();
        let out = outputs.port(0);
        while let Some(&(frame, msg)) = self.events.get(self.next)
            && frame < end
        {
            out.push(MidiEvent {
                frame: (frame - self.block_start) as u32,
                msg,
            });
            self.next += 1;
        }
        self.block_start = end;
    }
    fn ports(&self) -> &Ports {
        &self.ports
    }
}

struct StreamSource {
    signal: SignalFn,
    frame: u64,
    ports: Ports,
}

impl StreamSource {
    fn new(signal: SignalFn) -> Self {
        Self {
            signal,
            frame: 0,
            ports: PortBuilder::default().control_out(1).build(),
        }
    }
}

impl Node for StreamSource {
    fn process(&mut self, _: &mut AudioContext, _: &Inputs, outputs: &mut [&mut [f32]]) {
        for y in outputs[0].iter_mut() {
            *y = (self.signal)(self.frame);
            self.frame += 1;
        }
    }
    fn ports(&self) -> &Ports {
        &self.ports
    }
}

/// Panics if a note starts while already sounding, or a note-off ends nothing.
pub fn assert_well_formed(events: &[TimedMidi]) {
    let mut sounding = [0u128; 16];
    for (frame, msg) in events {
        let chan = &mut sounding[msg.channel_idx as usize & 0x0F];
        match msg.data {
            MidiMessageKind::NoteOn { note, .. } => {
                assert!(
                    *chan & (1 << note) == 0,
                    "note {note} on channel {} started at {frame} while already sounding",
                    msg.channel_idx
                );
                *chan |= 1 << note;
            }
            MidiMessageKind::NoteOff { note, .. } => {
                assert!(
                    *chan & (1 << note) != 0,
                    "note-off for {note} on channel {} at {frame} ends nothing",
                    msg.channel_idx
                );
                *chan &= !(1 << note);
            }
            _ => {}
        }
    }
}

/// (channel, note) of every note still sounding after `events`.
pub fn sounding_after(events: &[TimedMidi]) -> Vec<(u8, u8)> {
    let mut sounding = Vec::new();
    for (_, msg) in events {
        let chan = msg.channel_idx;
        match msg.data {
            MidiMessageKind::NoteOn { note, .. } => sounding.push((chan, note)),
            MidiMessageKind::NoteOff { note, .. } => sounding.retain(|&k| k != (chan, note)),
            _ => {}
        }
    }
    sounding
}
