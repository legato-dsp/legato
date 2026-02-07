use std::{
    fmt::Debug,
    sync::Arc,
    thread::JoinHandle,
    time::{Duration, Instant},
};

use crossbeam::{
    channel::{Receiver, Sender, bounded},
    select,
};
use midir::{Ignore, MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};

pub type MidiProducer = Sender<(MidiMessage, Instant)>;
pub type MidiReceiver = Receiver<(MidiMessage, Instant)>;

/// A number of errors that can occur when parsing midi messages
#[derive(Debug, Clone, PartialEq)]
pub enum MidiError {
    CouldNotParse,
    NotImplemented,
    InvalidPort,
    RingbufferFull,
    ConnectionError(String),
    SendError(String),
}

#[derive(Clone, Copy, PartialEq)]
pub struct PitchBend(u16);

impl PitchBend {
    /// Convert the "u14" midi pitch bend to a -8192 -> 8191 range i16
    pub fn as_i16(&self) -> i16 {
        self.0 as i16 - 8192
    }
    /// Convert the "u14" midi pitch bend to a -1.0 to 1.0 range
    pub fn as_normalized(&self) -> f32 {
        self.as_i16() as f32 / 8192.0
    }
}

impl Debug for PitchBend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.as_i16().to_string())
    }
}

// Prefixes for various functionality

const NOTE_ON: u8 = 0x9;
const NOTE_OFF: u8 = 0x8;
const CONTROL: u8 = 0xB;
const PITCH_WHEEL: u8 = 0xE;
const CHANNEL_AFTER_TOUCH: u8 = 0xD;
const POLYPHONIC_AFTER_TOUCH: u8 = 0xA;

const SYSTEM_PREFIX: u8 = 0xF;
const START: u8 = 0xFA;
const CONTINUE: u8 = 0xFB;
const STOP: u8 = 0xFC;
const CLOCK: u8 = 0xF8;
const SONG_POSITION_POINTER: u8 = 0xF2;

/// Limited subset of midi functionality for now.
///
/// Spec taken from this nice overview: https://github.com/mixxxdj/mixxx/wiki/MIDI-Crash-Course
///
/// Currently not using more complicated system messages as this may require
/// a dedicated parser and is a bit more complicated. Basically, some messages
/// are a flag to start capturing everything in-between the start and end flag,
/// and pass it to the system. This is useful for presets, updates, etc, but is
/// a bit beyond the scope here for the time being.
#[derive(Debug, Clone, PartialEq)]
pub enum MidiMessageKind {
    NoteOn { note: u8, velocity: u8 },
    NoteOff { note: u8, velocity: u8 },
    // After touch
    ChannelAftertouch { amount: u8 },
    PolyphonicAftertouch { note: u8, amount: u8 },
    Control { control_number: u8, value: u8 },
    PitchWheel { shift: PitchBend },
    // Basic clock functionality
    // TODO: Do we make these nodes? Do we make these control the runtime?
    Start,
    Stop,
    Clock,
    Continue,
    SongPositionPointer { value: u16 },
    // Dummy Message Only Used for Preallocated
    Dummy,
}

pub struct MidiListener {
    producer: MidiProducer,
}

impl MidiListener {
    pub fn new(producer: MidiProducer) -> Self {
        Self { producer }
    }
    /// Send a midi message to the midi store
    pub fn send_to_store(&mut self, msg: MidiMessage, instant: Instant) -> Result<(), MidiError> {
        self.producer
            .try_send((msg, instant))
            .map_err(|_| MidiError::RingbufferFull)
    }
}

pub struct MidiWriter {
    receiver: MidiReceiver,
}

impl MidiWriter {
    pub fn new(receiver: MidiReceiver) -> Self {
        Self { receiver }
    }
    pub fn send_to_midi_output(
        &mut self,
        msg: MidiMessage,
        connection: &mut MidiOutputConnection,
    ) -> Result<(), MidiError> {
        let encoded = msg.encode();
        let sliced = &encoded.data[..encoded.len];
        connection
            .send(sliced)
            .map_err(|x| MidiError::SendError(x.to_string()))
    }
    /// Drain the incoming messages and send to the system.
    /// You will most likely want to run this on a dedicated thread.
    ///
    /// TODO: Use some sort of timing to eliminate jitter.
    pub fn run(mut self, connection: &mut MidiOutputConnection) {
        loop {
            select! {
                recv(self.receiver) -> msg => {
                    if let Ok(inner) = msg {
                        let _ = self.send_to_midi_output(inner.0, connection);
                    }
                }
            }
        }
    }
}

/// A small struct to store the system -> audio offset from midi and hide it behind a nice interface.
///
/// When constructed, it uses the instant of that point, as well as that first midi message data.
pub struct MidiOffsetStore {
    sync_point: (Instant, u64),
}

impl MidiOffsetStore {
    /// Note: The time in which this is constructed matters.
    pub fn new(first_midi_micros: u64) -> Self {
        Self {
            sync_point: (Instant::now(), first_midi_micros),
        }
    }

    pub fn update(&mut self, midi_micros: u64) {
        self.sync_point.0 = Instant::now();
        self.sync_point.1 = midi_micros;
    }

    /// Get the instant value from the midi message
    pub fn to_instant(&self, midi_micros: u64) -> Instant {
        let (anchor_inst, anchor_micros) = self.sync_point;

        if midi_micros >= anchor_micros {
            anchor_inst + Duration::from_micros(midi_micros - anchor_micros)
        } else {
            anchor_inst
                .checked_sub(Duration::from_micros(anchor_micros - midi_micros))
                .unwrap_or(anchor_inst)
        }
    }
}

/// A small struct to send messages to the midi-writer
pub struct MidiWriterFrontend {
    producer: MidiProducer,
}

impl MidiWriterFrontend {
    pub fn new(producer: MidiProducer) -> Self {
        Self { producer }
    }
    /// Send a message from the store to the midi runtime to be executed on the system
    #[inline(always)]
    pub fn send_to_system_midi(&self, msg: MidiMessage, instant: Instant) -> Result<(), MidiError> {
        self.producer
            .try_send((msg, instant))
            .map_err(|x| MidiError::SendError(x.to_string()))
    }
}

const MIDI_CHANS: usize = 16;

#[derive(Clone)]
/// The MidiStore stores Midi messages in a flat layout.
///
/// So, channel 0 is 0..per_chan_cap, 1 is per_chan_cap..2*per_chan_cap, etc.
pub struct MidiStore {
    channel_messages: Vec<MidiMessage>,
    channel_messages_count: [usize; MIDI_CHANS],
    general_messages: Vec<MidiMessage>,
    general_messages_count: usize,
    capacity: usize,
}

fn get_dummy_midi() -> MidiMessage {
    MidiMessage {
        channel_idx: 0,
        data: MidiMessageKind::Dummy,
        instant: Instant::now(),
    }
}

impl MidiStore {
    pub fn new(capacity: usize) -> Self {
        Self {
            channel_messages: vec![get_dummy_midi(); capacity * MIDI_CHANS],
            channel_messages_count: [0; MIDI_CHANS],
            general_messages: vec![get_dummy_midi(); capacity],
            general_messages_count: 0,
            capacity,
        }
    }
    /// Clear the messages. We still have the same underlying allocation.
    pub fn clear(&mut self) {
        self.channel_messages_count = [0; MIDI_CHANS];
        self.general_messages_count = 0;
    }

    #[inline(always)]
    pub fn insert(&mut self, msg: MidiMessage) -> Result<(), MidiError> {
        let chan = msg.channel_idx as usize;
        match msg.data {
            // Channel messages
            MidiMessageKind::NoteOn { .. }
            | MidiMessageKind::NoteOff { .. }
            | MidiMessageKind::Control { .. }
            | MidiMessageKind::PolyphonicAftertouch { .. }
            | MidiMessageKind::ChannelAftertouch { .. }
            | MidiMessageKind::PitchWheel { .. } => {
                if chan >= MIDI_CHANS {
                    return Err(MidiError::InvalidPort);
                }

                let count = self.channel_messages_count[chan];
                if count >= self.capacity {
                    return Err(MidiError::RingbufferFull);
                }

                let index = chan * self.capacity + count;
                self.channel_messages[index] = msg;
                self.channel_messages_count[chan] += 1;

                Ok(())
            }
            MidiMessageKind::Start
            | MidiMessageKind::Continue
            | MidiMessageKind::Clock
            | MidiMessageKind::Stop
            | MidiMessageKind::SongPositionPointer { .. } => {
                let count = self.general_messages_count;
                if count >= self.capacity {
                    return Err(MidiError::RingbufferFull);
                }

                self.general_messages[count] = msg;
                self.general_messages_count += 1;

                Ok(())
            }
            MidiMessageKind::Dummy => unreachable!(),
        }
    }
    pub fn get_channel(&self, chan: usize) -> &[MidiMessage] {
        debug_assert!(chan < MIDI_CHANS);

        let start = self.capacity * chan;
        let count = self.channel_messages_count[chan];

        &self.channel_messages[start..start + count]
    }
    pub fn get_general(&self) -> &[MidiMessage] {
        &self.general_messages
    }
}

pub struct MidiRuntimeFrontend {
    _reader_handle: MidiInputConnection<()>,
    _writer_handle: JoinHandle<()>,
    writer_frontend: Arc<MidiWriterFrontend>,
    reader_consumer: MidiReceiver,
}

impl MidiRuntimeFrontend {
    pub fn new(
        reader_handle: MidiInputConnection<()>,
        writer_handle: JoinHandle<()>,
        writer_frontend: Arc<MidiWriterFrontend>,
        consumer: MidiReceiver,
    ) -> Self {
        Self {
            _reader_handle: reader_handle,
            _writer_handle: writer_handle,
            writer_frontend,
            reader_consumer: consumer,
        }
    }
    #[inline(always)]
    pub fn send(&mut self, msg: MidiMessage) -> Result<(), MidiError> {
        self.writer_frontend
            .send_to_system_midi(msg, Instant::now())
    }
    #[inline(always)]
    pub fn recv(&self) -> Option<MidiMessage> {
        self.reader_consumer.try_recv().ok().map(|x| x.0)
    }
}

pub fn start_midi_thread(
    capacity: usize,
    client_name: &'static str,
    in_port: MidiPortKind,
    out_port: MidiPortKind,
    port_name: &'static str,
) -> Result<(MidiRuntimeFrontend, Arc<MidiWriterFrontend>), MidiError> {
    // Setup the MidiInput with our client name
    let mut input = MidiInput::new(client_name).expect("Could not create MidiInput device!");
    // Ignore unsupported messages
    input.ignore(Ignore::SysexAndActiveSense);

    let in_ports = input.ports();
    // Find our port index from the enum passed in
    let in_port_index = in_port
        .select_port_in(&input)
        .expect("Could not create input port!");

    let input_port = &in_ports[in_port_index];

    // These are the channels that signal from reader -> store and store -> writer
    let (midi_reader_prod, midi_reader_consumer) = bounded::<(MidiMessage, Instant)>(capacity);
    let (midi_writer_prod, midi_writer_consumer) = bounded::<(MidiMessage, Instant)>(capacity);

    let mut midi_listener = MidiListener::new(midi_reader_prod);

    // The input connection thread
    let reader_handle = input
        .connect(
            input_port,
            port_name,
            move |_, message, _| {
                let instant = Instant::now();

                if let Ok(msg) = parse_midi(message, instant) {
                    // Init midi offset if not yet set
                    // TODO: Proper app wide error handling
                    if midi_listener.send_to_store(msg, instant).is_err() {
                        eprintln!("MIDI DROP");
                    }
                }
            },
            (),
        )
        .map_err(|x| MidiError::ConnectionError(x.to_string()))?;

    let output = MidiOutput::new(client_name).expect("Could not create MidiOutput device!");

    let midi_writer = MidiWriter::new(midi_writer_consumer);

    let out_ports = output.ports();
    // Create output port
    let out_port_index = out_port
        .select_port_out(&output)
        .expect("Could not create input port!");

    let output_port = &out_ports[out_port_index];

    let mut output_connection = output
        .connect(output_port, port_name)
        .map_err(|x| MidiError::ConnectionError(x.to_string()))?;

    // Spawning the writer thread, we keep the handle as we will use this on the MidiRuntime struct
    let writer_handle = std::thread::spawn(move || {
        midi_writer.run(&mut output_connection);
    });

    let writer_frontend = Arc::new(MidiWriterFrontend::new(midi_writer_prod));

    // Assemble the final midi runtime.
    let runtime = MidiRuntimeFrontend::new(
        reader_handle,
        writer_handle,
        writer_frontend.clone(),
        midi_reader_consumer,
    );

    Ok((runtime, writer_frontend))
}

/// A small struct to easily create variable slices of our midi
/// down the line without heap allocation. This is because
/// not all midi messages are the same length.
///
/// We can use the len to slice, which provides the proper shape
/// for the Midir crate.
pub struct EncodedMidi {
    pub data: [u8; 3],
    pub len: usize,
}

/// A minimal struct wrapping the message kind and targeted channel
#[derive(Debug, Clone, PartialEq)]
pub struct MidiMessage {
    pub data: MidiMessageKind,
    pub instant: Instant,
    pub channel_idx: u8,
}

impl MidiMessage {
    pub fn encode(&self) -> EncodedMidi {
        match self.data {
            MidiMessageKind::NoteOn { note, velocity } => {
                let mut data = [0_u8; 3];
                data[0] = (NOTE_ON << 4) | (self.channel_idx & 0x0F);
                data[1] = note;
                data[2] = velocity;

                EncodedMidi { data, len: 3 }
            }
            MidiMessageKind::NoteOff { note, velocity } => {
                let mut data = [0_u8; 3];
                data[0] = (NOTE_OFF << 4) | (self.channel_idx & 0x0F);
                data[1] = note;
                data[2] = velocity;

                EncodedMidi { data, len: 3 }
            }
            MidiMessageKind::Control {
                control_number,
                value,
            } => {
                let mut data = [0_u8; 3];
                data[0] = (CONTROL << 4) | (self.channel_idx & 0x0F);
                data[1] = control_number;
                data[2] = value;

                EncodedMidi { data, len: 3 }
            }
            MidiMessageKind::PitchWheel { shift } => {
                let mut data = [0_u8; 3];
                data[0] = (PITCH_WHEEL << 4) | (self.channel_idx & 0x0F);
                // get the underlying u16
                let value = shift.0;
                // LSB
                data[1] = (value & 0x7F) as u8;
                // MSB
                data[2] = ((value >> 7) & 0x7F) as u8;

                EncodedMidi { data, len: 3 }
            }
            MidiMessageKind::ChannelAftertouch { amount } => {
                let mut data = [0_u8; 3];
                data[0] = (CHANNEL_AFTER_TOUCH << 4) | (self.channel_idx & 0x0F);

                data[1] = amount;

                EncodedMidi { data, len: 2 }
            }
            MidiMessageKind::PolyphonicAftertouch { note, amount } => {
                let mut data = [0_u8; 3];
                data[0] = (POLYPHONIC_AFTER_TOUCH << 4) | (self.channel_idx & 0x0F);

                data[1] = note;
                data[2] = amount;

                EncodedMidi { data, len: 3 }
            }
            MidiMessageKind::Start => EncodedMidi {
                data: [START, 0, 0],
                len: 1,
            },
            MidiMessageKind::Continue => EncodedMidi {
                data: [CONTINUE, 0, 0],
                len: 1,
            },
            MidiMessageKind::Stop => EncodedMidi {
                data: [STOP, 0, 0],
                len: 1,
            },
            MidiMessageKind::Clock => EncodedMidi {
                data: [CLOCK, 0, 0],
                len: 1,
            },
            MidiMessageKind::SongPositionPointer { value } => {
                let mut data = [0_u8; 3];
                data[0] = SONG_POSITION_POINTER;
                // LSB
                data[1] = (value & 0x7F) as u8;
                // MSB
                data[2] = ((value >> 7) & 0x7F) as u8;

                EncodedMidi { data, len: 3 }
            }
            MidiMessageKind::Dummy => unreachable!(),
        }
    }
}

pub fn parse_midi(message: &[u8], instant: Instant) -> Result<MidiMessage, MidiError> {
    if message.is_empty() {
        return Err(MidiError::CouldNotParse);
    }

    let message_byte = message[0];

    let message_kind = message_byte >> 4;
    let message_channel_index = message_byte & 0x0F;

    match message_kind {
        NOTE_ON => {
            let note = message[1];
            let velocity = message[2];

            let data = MidiMessageKind::NoteOn { note, velocity };

            Ok(MidiMessage {
                channel_idx: message_channel_index,
                instant,
                data,
            })
        }
        NOTE_OFF => {
            let note = message[1];
            let velocity = message[2];
            let data = MidiMessageKind::NoteOff { note, velocity };

            Ok(MidiMessage {
                channel_idx: message_channel_index,
                instant,
                data,
            })
        }
        CONTROL => {
            let control_number = message[1];
            let value = message[2];
            let data = MidiMessageKind::Control {
                control_number,
                value,
            };

            Ok(MidiMessage {
                data,
                instant,
                channel_idx: message_channel_index,
            })
        }
        PITCH_WHEEL => {
            let lsb = message[1];
            let msb = message[2];

            let value = ((msb as u16) << 7) | (lsb as u16 & 0x7F);
            let data = MidiMessageKind::PitchWheel {
                shift: PitchBend(value),
            };

            Ok(MidiMessage {
                data,
                instant,
                channel_idx: message_channel_index,
            })
        }
        CHANNEL_AFTER_TOUCH => {
            let amount = message[1];
            let data = MidiMessageKind::ChannelAftertouch { amount };

            Ok(MidiMessage {
                data,
                instant,
                channel_idx: message_channel_index,
            })
        }
        POLYPHONIC_AFTER_TOUCH => {
            let note = message[1];
            let amount = message[2];

            let data = MidiMessageKind::PolyphonicAftertouch { note, amount };

            Ok(MidiMessage {
                data,
                instant,
                channel_idx: message_channel_index,
            })
        }
        SYSTEM_PREFIX => {
            let data = match message_byte {
                START => Ok(MidiMessageKind::Start),
                CONTINUE => Ok(MidiMessageKind::Continue),
                STOP => Ok(MidiMessageKind::Stop),
                CLOCK => Ok(MidiMessageKind::Clock),
                SONG_POSITION_POINTER => {
                    let lsb = message[1];
                    let msb = message[2];

                    let value = ((msb as u16) << 7) | (lsb as u16 & 0x7F);

                    Ok(MidiMessageKind::SongPositionPointer { value })
                }
                _ => Err(MidiError::NotImplemented),
            }?;

            Ok(MidiMessage {
                data,
                instant,
                channel_idx: 0,
            })
        }
        _ => Err(MidiError::NotImplemented),
    }
}

#[derive(Default, Clone, PartialEq)]
pub enum MidiPortKind {
    #[default]
    Default,
    Index(usize),
    Named(&'static str),
}

impl MidiPortKind {
    pub fn select_port_in(&self, midi_input: &MidiInput) -> Result<usize, MidiError> {
        match self {
            MidiPortKind::Default => Ok(0),
            MidiPortKind::Index(i) => Ok(*i),
            MidiPortKind::Named(name) => {
                if let Some(idx) = midi_input
                    .ports()
                    .iter()
                    .position(|x| midi_input.port_name(x).unwrap() == *name)
                {
                    Ok(idx)
                } else {
                    Err(MidiError::InvalidPort)
                }
            }
        }
    }
    pub fn select_port_out(&self, midi_output: &MidiOutput) -> Result<usize, MidiError> {
        match self {
            MidiPortKind::Default => Ok(0),
            MidiPortKind::Index(i) => Ok(*i),
            MidiPortKind::Named(name) => {
                if let Some(idx) = midi_output
                    .ports()
                    .iter()
                    .position(|x| midi_output.port_name(x).unwrap() == *name)
                {
                    Ok(idx)
                } else {
                    Err(MidiError::InvalidPort)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Small helper to just encode, re-encode, and assert that they are the same
    fn assert_can_reconstruct(msg: MidiMessage) {
        let encoded = msg.encode();
        let decoded = parse_midi(&encoded.data[..encoded.len as usize], Instant::now())
            .expect("Failed to decode message!");
        assert_eq!(msg.data, decoded.data);
        assert_eq!(msg.channel_idx, decoded.channel_idx);
    }

    #[test]
    fn test_note_on_off() {
        for channel in 0..16 {
            let note_on = MidiMessage {
                channel_idx: channel,
                instant: Instant::now(),
                data: MidiMessageKind::NoteOn {
                    note: 60,
                    velocity: 100,
                },
            };
            assert_can_reconstruct(note_on);

            let note_off = MidiMessage {
                channel_idx: channel,
                instant: Instant::now(),
                data: MidiMessageKind::NoteOff {
                    note: 60,
                    velocity: 50,
                },
            };
            assert_can_reconstruct(note_off);
        }
    }

    #[test]
    fn test_control_change() {
        for channel in 0..16 {
            let msg = MidiMessage {
                channel_idx: channel,
                instant: Instant::now(),
                data: MidiMessageKind::Control {
                    control_number: 10,
                    value: 127,
                },
            };
            assert_can_reconstruct(msg);
        }
    }

    #[test]
    fn test_pitch_wheel() {
        for channel in 0..16 {
            let msg = MidiMessage {
                channel_idx: channel,
                instant: Instant::now(),
                data: MidiMessageKind::PitchWheel {
                    shift: PitchBend(8192),
                },
            };
            assert_can_reconstruct(msg);

            let msg_low = MidiMessage {
                channel_idx: channel,
                instant: Instant::now(),
                data: MidiMessageKind::PitchWheel {
                    shift: PitchBend(0),
                },
            };
            assert_can_reconstruct(msg_low);

            let msg_high = MidiMessage {
                channel_idx: channel,
                instant: Instant::now(),
                data: MidiMessageKind::PitchWheel {
                    shift: PitchBend(16383),
                },
            };
            assert_can_reconstruct(msg_high);
        }
    }

    #[test]
    fn test_aftertouch() {
        for channel in 0..16 {
            let channel_at = MidiMessage {
                channel_idx: channel,
                instant: Instant::now(),
                data: MidiMessageKind::ChannelAftertouch { amount: 64 },
            };
            assert_can_reconstruct(channel_at);

            let poly_at = MidiMessage {
                channel_idx: channel,
                instant: Instant::now(),
                data: MidiMessageKind::PolyphonicAftertouch {
                    note: 60,
                    amount: 127,
                },
            };
            assert_can_reconstruct(poly_at);
        }
    }

    #[test]
    fn test_system_messages() {
        let start = MidiMessage {
            channel_idx: 0,
            instant: Instant::now(),
            data: MidiMessageKind::Start,
        };
        assert_can_reconstruct(start);

        let continue_msg = MidiMessage {
            channel_idx: 0,
            instant: Instant::now(),
            data: MidiMessageKind::Continue,
        };
        assert_can_reconstruct(continue_msg);

        let stop = MidiMessage {
            channel_idx: 0,
            instant: Instant::now(),
            data: MidiMessageKind::Stop,
        };
        assert_can_reconstruct(stop);

        let clock = MidiMessage {
            channel_idx: 0,
            instant: Instant::now(),
            data: MidiMessageKind::Clock,
        };
        assert_can_reconstruct(clock);

        let spp = MidiMessage {
            channel_idx: 0,
            instant: Instant::now(),
            data: MidiMessageKind::SongPositionPointer { value: 0x1234 },
        };
        assert_can_reconstruct(spp);
    }

    #[test]
    fn test_invalid_parse() {
        let empty: &[u8] = &[];
        assert!(parse_midi(empty, Instant::now()).is_err());

        let unknown: &[u8] = &[0xFF];
        assert!(matches!(
            parse_midi(unknown, Instant::now()),
            Err(MidiError::NotImplemented)
        ));
    }
}
