use generic_array::{sequence::GenericSequence, ArrayLength, GenericArray};

use crate::engine::port::{AudioInputPort, AudioOutputPort, PortMeta};

/// Utility function for generating audio input ports for nodes
pub fn generate_audio_inputs<Ai>() -> GenericArray<AudioInputPort, Ai>
where
    Ai: ArrayLength,
{
    GenericArray::generate(|i| AudioInputPort {
        meta: {
            PortMeta {
                name: match Ai::USIZE {
                    1 => "in",
                    2 => {
                        if i == 0 {
                            "l"
                        } else {
                            "r"
                        }
                    }
                    _ => "in",
                },
                index: i,
            }
        },
    })
}

/// Utility function for generating audio output ports for nodes
pub fn generate_audio_outputs<Ao>() -> GenericArray<AudioOutputPort, Ao>
where
    Ao: ArrayLength,
{
    GenericArray::generate(|i| AudioOutputPort {
        meta: {
            PortMeta {
                name: match Ao::USIZE {
                    1 => "out",
                    2 => {
                        if i == 0 {
                            "l"
                        } else {
                            "r"
                        }
                    }
                    _ => "out",
                },
                index: i,
            }
        },
    })
}
