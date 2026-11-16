//! Storage for every MIDI port in the graph, and the view a node gets of its own ports.
//!
//! Each MIDI output port owns a [`MidiBuffer`]. Before a node runs, the executor
//! copies what its sources wrote into that node's input buffers, clears its
//! outputs, and binds them, so [`AudioContext::node_midi`] returns exactly
//! that node's ports.
//!
//! [`AudioContext::node_midi`]: crate::context::AudioContext::node_midi

use crate::midi::{MidiBuffer, MidiEvent, MidiMessage, MidiMessageKind, MidiStore};

/// Events one MIDI port can hold per block.
pub const MIDI_BUFFER_CAPACITY: usize = 1024;

/// MIDI inputs a single node can declare.
pub const MAX_MIDI_INPUTS: usize = 8;

/// The MIDI ports of the node currently being processed.
pub struct NodeMidi<'a> {
    /// Copies of what this node's sources wrote this block, merged in frame order.
    pub inputs: MidiInputs<'a>,
    /// This node's own outputs, empty at the start of each block.
    pub outputs: MidiOutputs<'a>,
}

pub struct MidiInputs<'a> {
    ports: &'a [MidiBuffer],
}

impl<'a> MidiInputs<'a> {
    pub fn port(&self, index: usize) -> &'a [MidiEvent] {
        self.ports[index].events()
    }

    pub fn len(&self) -> usize {
        self.ports.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ports.is_empty()
    }
}

pub struct MidiOutputs<'a> {
    ports: &'a mut [MidiBuffer],
}

impl MidiOutputs<'_> {
    pub fn port(&mut self, index: usize) -> &mut MidiBuffer {
        &mut self.ports[index]
    }

    pub fn ports_mut(&mut self) -> &mut [MidiBuffer] {
        self.ports
    }

    pub fn len(&self) -> usize {
        self.ports.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ports.is_empty()
    }
}

/// One bit per (channel, note) that an output has turned on and not yet off.
#[derive(Clone, Copy, Default)]
struct Sounding([u128; 16]);

impl Sounding {
    fn track(&mut self, event: &MidiEvent) {
        let chan = &mut self.0[event.msg.channel_idx as usize & 0x0F];
        match event.msg.data {
            MidiMessageKind::NoteOn { note, .. } => *chan |= 1 << (note & 0x7F),
            MidiMessageKind::NoteOff { note, .. } => *chan &= !(1 << (note & 0x7F)),
            _ => {}
        }
    }

    /// Writes a note-off at frame 0 for every sounding note, ahead of the block's other events.
    fn release_into(&mut self, buffer: &mut MidiBuffer) {
        for (channel_idx, notes) in self.0.iter_mut().enumerate().rev() {
            for note in (0..128u8).rev().filter(|&n| *notes & (1 << n) != 0) {
                buffer.push_first(MidiEvent {
                    frame: 0,
                    msg: MidiMessage {
                        channel_idx: channel_idx as u8,
                        data: MidiMessageKind::NoteOff { note, velocity: 0 },
                    },
                });
            }
            *notes = 0;
        }
    }
}

/// Where the bound node's outputs sit in [`MidiBus::outputs`], and how many inputs it has.
#[derive(Clone, Copy, Default)]
struct Binding {
    outputs_start: usize,
    outputs_len: usize,
    inputs_len: usize,
}

pub(crate) struct MidiBus {
    /// One buffer per MIDI output port in the graph, laid out by the executor.
    outputs: Box<[MidiBuffer]>,
    sounding: Box<[Sounding]>,
    /// Scratch for the bound node's inputs, reused by every node.
    inputs: Box<[MidiBuffer]>,
    bound: Binding,
}

impl Default for MidiBus {
    fn default() -> Self {
        Self {
            outputs: Box::default(),
            sounding: Box::default(),
            inputs: vec![MidiBuffer::with_capacity(MIDI_BUFFER_CAPACITY); MAX_MIDI_INPUTS].into(),
            bound: Binding::default(),
        }
    }
}

impl MidiBus {
    /// Not realtime safe.
    pub(crate) fn prepare(&mut self, output_ports: usize) {
        self.outputs = vec![MidiBuffer::with_capacity(MIDI_BUFFER_CAPACITY); output_ports].into();
        self.sounding = vec![Sounding::default(); output_ports].into();
        self.bound = Binding::default();
    }

    pub(crate) fn output_ports(&self) -> usize {
        self.outputs.len()
    }

    /// Clears the node's outputs and inputs, and makes them what [`MidiBus::node_midi`] returns.
    pub(crate) fn bind(&mut self, outputs_start: usize, outputs_len: usize, inputs_len: usize) {
        self.bound = Binding {
            outputs_start,
            outputs_len,
            inputs_len,
        };
        self.bound_outputs().iter_mut().for_each(MidiBuffer::clear);
        self.inputs[..inputs_len]
            .iter_mut()
            .for_each(MidiBuffer::clear);
    }

    pub(crate) fn unbind(&mut self) {
        self.bound = Binding::default();
    }

    /// Merges the output at `source_slot` into the bound node's input `input`.
    pub(crate) fn gather(&mut self, input: usize, source_slot: usize) {
        let Self {
            outputs, inputs, ..
        } = self;
        for &event in outputs[source_slot].events() {
            inputs[input].push(event);
        }
    }

    pub(crate) fn gather_host(&mut self, input: usize, store: &MidiStore) {
        store.merge_into(&mut self.inputs[input]);
    }

    pub(crate) fn clear_bound_outputs(&mut self) {
        self.bound_outputs().iter_mut().for_each(MidiBuffer::clear);
    }

    /// Copies the bound node's input `input` to its output `output`.
    pub(crate) fn pass_through(&mut self, input: usize, output: usize) {
        let start = self.bound.outputs_start;
        let Self {
            outputs, inputs, ..
        } = self;
        for &event in inputs[input].events() {
            outputs[start + output].push(event);
        }
    }

    /// Ends every note the bound node's outputs left sounding.
    pub(crate) fn release_bound(&mut self) {
        let range = self.bound_range();
        for (buffer, sounding) in self.outputs[range.clone()]
            .iter_mut()
            .zip(&mut self.sounding[range])
        {
            sounding.release_into(buffer);
        }
    }

    /// Records which notes the bound node's outputs leave sounding after this block.
    pub(crate) fn track_bound(&mut self) {
        let range = self.bound_range();
        for (buffer, sounding) in self.outputs[range.clone()]
            .iter()
            .zip(&mut self.sounding[range])
        {
            buffer.events().iter().for_each(|e| sounding.track(e));
        }
    }

    pub(crate) fn output(&self, slot: usize) -> &MidiBuffer {
        &self.outputs[slot]
    }

    pub(crate) fn node_midi(&mut self) -> NodeMidi<'_> {
        let Binding {
            outputs_start,
            outputs_len,
            inputs_len,
        } = self.bound;
        NodeMidi {
            inputs: MidiInputs {
                ports: &self.inputs[..inputs_len],
            },
            outputs: MidiOutputs {
                ports: &mut self.outputs[outputs_start..outputs_start + outputs_len],
            },
        }
    }

    fn bound_outputs(&mut self) -> &mut [MidiBuffer] {
        let range = self.bound_range();
        &mut self.outputs[range]
    }

    fn bound_range(&self) -> std::ops::Range<usize> {
        self.bound.outputs_start..self.bound.outputs_start + self.bound.outputs_len
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(frame: u32, channel_idx: u8, data: MidiMessageKind) -> MidiEvent {
        MidiEvent {
            frame,
            msg: MidiMessage { data, channel_idx },
        }
    }

    fn on(note: u8) -> MidiMessageKind {
        MidiMessageKind::NoteOn { note, velocity: 1 }
    }

    fn off(note: u8) -> MidiMessageKind {
        MidiMessageKind::NoteOff { note, velocity: 0 }
    }

    #[test]
    fn release_ends_only_notes_still_sounding_ahead_of_other_events() {
        let mut sounding = Sounding::default();
        for e in [
            note(0, 0, on(60)),
            note(1, 0, on(62)),
            note(2, 0, off(60)),
            note(3, 9, on(127)),
        ] {
            sounding.track(&e);
        }

        let mut buffer = MidiBuffer::with_capacity(8);
        buffer.push(note(0, 0, on(64)));
        sounding.release_into(&mut buffer);

        let events: Vec<_> = buffer
            .events()
            .iter()
            .map(|e| (e.frame, e.msg.channel_idx, e.msg.data))
            .collect();
        assert_eq!(events, [(0, 0, off(62)), (0, 9, off(127)), (0, 0, on(64))]);
        assert_eq!(sounding.0, [0; 16]);
    }
}
