use std::{fmt::Debug, sync::Arc};

use ringbuf::traits::{Consumer, Producer};
use ringbuf::{HeapRb, SharedRb, storage::Heap, traits::Split, wrap::caching::Caching};

pub type MidiProducer = Caching<Arc<SharedRb<Heap<MidiMessage>>>, true, false>;
pub type MidiConsumer = Caching<Arc<SharedRb<Heap<MidiMessage>>>, false, true>;

/// A number of errors that can occur when parsing midi messages
#[derive(Debug, Clone, PartialEq)]
pub enum MidiError {
    CouldNotParse,
    NotImplemented,
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

// Prefixes for various channels
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
/// Spec taken from https://github.com/mixxxdj/mixxx/wiki/MIDI-Crash-Course
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
    Start,
    Stop,
    Clock,
    Continue,
    SongPositionPointer { value: u16 },
}

/// The frontend to send MidiMessages to.
///
/// This should live on a different thread than the audio thread.
pub struct MidiFrontend {
    producer: MidiProducer,
}

impl MidiFrontend {
    pub fn new(producer: MidiProducer) -> Self {
        Self { producer }
    }
    pub fn send(&mut self, msg: MidiMessage) -> Result<(), MidiMessage> {
        self.producer.try_push(msg)
    }
}

/// The rt-safe backend that the audio graph uses for
/// to drain midi messages.
pub struct MidiBackend {
    consumer: MidiConsumer,
}

impl MidiBackend {
    pub fn new(consumer: MidiConsumer) -> Self {
        Self { consumer }
    }
    pub fn recv(&mut self) -> Option<&MidiMessage> {
        self.consumer.iter().next()
    }
}

pub fn build_midi(capacity: usize) -> (MidiFrontend, MidiBackend) {
    let (prod, cons) = HeapRb::<MidiMessage>::new(capacity).split();

    let frontend = MidiFrontend::new(prod);
    let backend = MidiBackend::new(cons);

    (frontend, backend)
}

/// A small struct to easily create variable slices of our midi
/// down the line without heap allocation. This is because
/// not all midi messages are the same length.
///
/// We can use the len to slice, which provides the proper shape
/// for the Midir crate.
pub struct EncodedMidi {
    pub data: [u8; 3],
    pub len: u8,
}

/// A minimal struct wrapping the message kind and targeted channel
#[derive(Debug, Clone, PartialEq)]
pub struct MidiMessage {
    pub data: MidiMessageKind,
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
        }
    }
}

impl TryFrom<&[u8]> for MidiMessage {
    type Error = MidiError;
    fn try_from(message: &[u8]) -> Result<Self, Self::Error> {
        if message.len() < 1 {
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
                    data,
                })
            }
            NOTE_OFF => {
                let note = message[1];
                let velocity = message[2];
                let data = MidiMessageKind::NoteOff { note, velocity };

                Ok(MidiMessage {
                    channel_idx: message_channel_index,
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
                    channel_idx: message_channel_index,
                })
            }
            CHANNEL_AFTER_TOUCH => {
                let amount = message[1];
                let data = MidiMessageKind::ChannelAftertouch { amount };

                Ok(MidiMessage {
                    data,
                    channel_idx: message_channel_index,
                })
            }
            POLYPHONIC_AFTER_TOUCH => {
                let note = message[1];
                let amount = message[2];

                let data = MidiMessageKind::PolyphonicAftertouch { note, amount };

                Ok(MidiMessage {
                    data,
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
                    channel_idx: 0,
                })
            }
            _ => Err(MidiError::NotImplemented),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Small helper to just encode, re-encode, and assert that they are the same
    fn assert_can_reconstruct(msg: MidiMessage) {
        let encoded = msg.encode();
        let decoded = MidiMessage::try_from(&encoded.data[..encoded.len as usize])
            .expect("Failed to decode message!");
        assert_eq!(msg, decoded);
    }

    #[test]
    fn test_note_on_off() {
        for channel in 0..16 {
            let note_on = MidiMessage {
                channel_idx: channel,
                data: MidiMessageKind::NoteOn {
                    note: 60,
                    velocity: 100,
                },
            };
            assert_can_reconstruct(note_on);

            let note_off = MidiMessage {
                channel_idx: channel,
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
                data: MidiMessageKind::PitchWheel {
                    shift: PitchBend(8192),
                },
            };
            assert_can_reconstruct(msg);

            let msg_low = MidiMessage {
                channel_idx: channel,
                data: MidiMessageKind::PitchWheel {
                    shift: PitchBend(0),
                },
            };
            assert_can_reconstruct(msg_low);

            let msg_high = MidiMessage {
                channel_idx: channel,
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
                data: MidiMessageKind::ChannelAftertouch { amount: 64 },
            };
            assert_can_reconstruct(channel_at);

            let poly_at = MidiMessage {
                channel_idx: channel,
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
            data: MidiMessageKind::Start,
        };
        assert_can_reconstruct(start);

        let continue_msg = MidiMessage {
            channel_idx: 0,
            data: MidiMessageKind::Continue,
        };
        assert_can_reconstruct(continue_msg);

        let stop = MidiMessage {
            channel_idx: 0,
            data: MidiMessageKind::Stop,
        };
        assert_can_reconstruct(stop);

        let clock = MidiMessage {
            channel_idx: 0,
            data: MidiMessageKind::Clock,
        };
        assert_can_reconstruct(clock);

        let spp = MidiMessage {
            channel_idx: 0,
            data: MidiMessageKind::SongPositionPointer { value: 0x1234 },
        };
        assert_can_reconstruct(spp);
    }

    #[test]
    fn test_invalid_parse() {
        let empty: &[u8] = &[];
        assert!(MidiMessage::try_from(empty).is_err());

        let unknown: &[u8] = &[0xFF];
        assert!(matches!(
            MidiMessage::try_from(unknown),
            Err(MidiError::NotImplemented)
        ));
    }
}
