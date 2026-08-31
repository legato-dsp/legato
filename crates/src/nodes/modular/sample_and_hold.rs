use std::time::Duration;

use crate::{
    builder::{ResourceBuilderView, ValidationError},
    context::AudioContext,
    dsl::ir::DSLParams,
    msg::{NodeMessage, RtValue},
    node::{DynNode, Inputs, Node},
    ports::{PortBuilder, Ports},
    spec::{NodeDefinition, NodeSpec},
};

#[derive(Clone)]
pub struct SampleAndHold {
    /// The sample rate, we are keeping this here to validate incoming params
    sr: u32,
    /// The current sampled value being sustained
    held: f32,
    /// The current hold duration, in samples
    hold_time_in_samples: u32,
    /// A counter of the number of samples held
    samples_held: u32,
    /// The port specification for [`SampleAndHold`]
    ports: Ports,
}

impl SampleAndHold {
    pub fn new(sr: u32, hold_time_in_samples: u32) -> Self {
        Self {
            held: 0.0,
            sr,
            hold_time_in_samples: hold_time_in_samples.max(1), // Default to one second at current sample rate
            samples_held: 0,
            ports: PortBuilder::default()
                .audio_in(1)
                .default_in()
                .audio_in_named(&["hold"])
                .audio_out(1)
                .default_out()
                .build(),
        }
    }
}

impl NodeDefinition for SampleAndHold {
    const NAME: &'static str = "sample_and_hold";
    const DESCRIPTION: &'static str = "A basic modular-style sample and hold node, the hold_time parameter determines how frequently it fetches new information.";
    const REQUIRED_PARAMS: &'static [&'static str] = &[];
    const OPTIONAL_PARAMS: &'static [&'static str] = &["hold_time"];

    fn create(
        rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        let sr = rb.config.sample_rate as u32;
        let hold_time = p
            .get_duration_ms("hold_time")
            .unwrap_or(Duration::from_secs(1));

        let hold_time_in_samples = hold_time.as_secs_f32() * sr as f32;

        Ok(Box::new(SampleAndHold::new(
            sr,
            hold_time_in_samples.floor() as u32,
        )))
    }
}

impl Node for SampleAndHold {
    fn process(&mut self, _: &mut AudioContext, inputs: &Inputs, outputs: &mut [&mut [f32]]) {
        let audio_out = &mut outputs[0];
        if let Some(audio_in) = inputs[0] {
            for (sample_in, sample_out) in audio_in.iter().zip(audio_out.iter_mut()) {
                self.samples_held += 1;
                // Check and see if we have iterated enough to update the held value
                if self.samples_held >= self.hold_time_in_samples {
                    self.samples_held = 0;
                    self.held = *sample_in;
                }

                *sample_out = self.held;
            }
        }
    }
    fn handle_msg(&mut self, msg: crate::msg::NodeMessage) {
        if let NodeMessage::SetParam(payload) = msg {
            let incoming = match (payload.param_name, payload.value) {
                ("hold", RtValue::U32(val)) => val as u32,
                ("hold", RtValue::I32(val)) => val as u32,
                ("hold", RtValue::F32(val)) => val.round() as u32, // TODO: Semantics?
                _ => unimplemented!("Incorrect parameter passed to SampleAndHold!"),
            };

            // Process handles the updating of any state, set minimum as 1 sample
            self.hold_time_in_samples = incoming.max(1);
        }
    }
    fn ports(&self) -> &Ports {
        &self.ports
    }
}

// #[cfg(test)]
// mod test {
//     use crate::config::Config;

//     /// Here we have 4 different values, one second apart.
//     /// We then sample every half second, and expect to see each one twice.
//     #[test]
//     fn test_basic_sample_and_hold() {
//         // Test input block, 4 blocks of 256, 256 sample rate
//         let input: [f32; 1024] = std::array::from_fn(|i| {
//             let nth_block_floored = i + 1 / 256;
//             nth_block_floored as f32
//         });

//         let output = [0.0; 1024];

//         let config = Config {
//             block_size: 256,
//             channels: 1,
//             rt_capacity: 0,
//             sample_rate: 256,
//         };

//         let node =

//         // We hold for one half second, with our convenient sample rate this is 128
//     }
// }
