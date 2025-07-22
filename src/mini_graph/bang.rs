#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Bang {
    Bang,
    BangF32(f32),
    BangMidi(MidiData), // Perhaps an enum in the future
    BangU32(u32),
    BangBool(bool),
    BangUSize(usize),
    SetParamU32(usize, u32),
    SetParamF32(usize, f32),
    SetParamBool(usize, bool),
    Empty,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MidiData {
    pub key: u8,
    pub vel: u8
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Midi {
    NoteOn(MidiData),
    NoteOff(MidiData),
    // TODO: Aftertouch, etc.
}