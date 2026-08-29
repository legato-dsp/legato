use crate::ports::{PortBuilder, Ports};

#[derive(Clone)]
pub struct SampleAndHold {
    /// The current sampled value being sustained
    val: f32,
    ports: Ports,
}

impl SampleAndHold {
    pub fn new() -> Self {
        Self {
            val: 0.0,
            ports: PortBuilder::default().audio_in(1).audio_out(1).build(),
        }
    }
}
