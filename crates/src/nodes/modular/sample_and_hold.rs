use std::time::Duration;

use crate::{
    builder::{ResourceBuilderView, ValidationError},
    context::AudioContext,
    dsl::ir::DSLParams,
    msg::{NodeMessage, RtValue},
    node::{DynNode, Inputs, Node},
    persample::PerSampleNode,
    ports::{PortBuilder, Ports},
    spec::NodeDefinition,
};

#[derive(Clone)]
pub struct SampleAndHold {
    /// The current sampled value being sustained
    held: f32,
    /// The current hold duration, in samples
    hold_time_in_samples: u32,
    /// A counter of the number of samples held
    samples_held: u32,
    /// Used to convert the hold_time modulation from ms to samples
    sr: f32,
    /// The port specification for [`SampleAndHold`]
    ports: Ports,
}

impl SampleAndHold {
    pub fn new(hold_time_in_samples: u32, sr: f32) -> Self {
        Self {
            held: 0.0,
            hold_time_in_samples: hold_time_in_samples.max(1), // Default to one second at current sample rate
            samples_held: 0,
            sr,
            ports: PortBuilder::default()
                .audio_in(1)
                .default_in()
                .audio_in_named(&["hold_time"])
                .audio_out(1)
                .default_out()
                .build(),
        }
    }
    #[inline(always)]
    fn tick_inner(&mut self, sample_in: f32, modulation: Option<f32>) -> f32 {
        if let Some(modulation) = modulation {
            // Cast modulation from ms to seconds, multiply by the sample_rate, and set a minimum of 1
            self.hold_time_in_samples = (((self.sr * (modulation / 1000.0)).floor()) as u32).max(1);
        }

        // To get the initial value, as well as an updated, we set samples_held to 0
        // This triggers the next branch, so we get the initial value as well
        if self.samples_held >= self.hold_time_in_samples {
            self.samples_held = 0;
        }

        if self.samples_held == 0 {
            self.held = sample_in;
        }

        self.samples_held += 1;

        self.held
    }
    #[inline]
    fn process_no_modulation(&mut self, inputs: &Inputs, outputs: &mut [&mut [f32]]) {
        let audio_out = &mut outputs[0];
        if let Some(audio_in) = inputs[0] {
            for (sample_in, sample_out) in audio_in.iter().zip(audio_out.iter_mut()) {
                *sample_out = self.tick_inner(*sample_in, None);
            }
        }
    }
    #[inline]
    fn process_with_modulation(&mut self, inputs: &Inputs, outputs: &mut [&mut [f32]]) {
        let audio_out = &mut outputs[0];
        let modulation = inputs[1].unwrap(); // Already checked in previous call-site

        if let Some(audio_in) = inputs[0] {
            for ((sample_in, modulation), sample_out) in
                audio_in.iter().zip(modulation).zip(audio_out.iter_mut())
            {
                *sample_out = self.tick_inner(*sample_in, Some(*modulation));
            }
        }
    }
}

impl SampleAndHold {
    pub fn from_params(
        rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Self, ValidationError> {
        let sr = rb.config.sample_rate as u32;
        let hold_time = p
            .get_duration_ms("hold_time")
            .unwrap_or(Duration::from_secs(1));

        let hold_time_in_samples = hold_time.as_secs_f32() * sr as f32;

        Ok(SampleAndHold::new(
            hold_time_in_samples.floor() as u32,
            sr as f32,
        ))
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
        Ok(Box::new(Self::from_params(rb, p)?))
    }
}

impl PerSampleNode for SampleAndHold {
    fn ports(&self) -> &Ports {
        &self.ports
    }

    fn tick(&mut self, in_frame: &[Option<f32>], out_frame: &mut [f32]) {
        out_frame[0] = self.tick_inner(in_frame[0].unwrap_or(0.0), in_frame[1]);
    }

    fn handle_msg(&mut self, msg: NodeMessage) {
        Node::handle_msg(self, msg);
    }
}

impl Node for SampleAndHold {
    fn process(&mut self, _ctx: &mut AudioContext, inputs: &Inputs, outputs: &mut [&mut [f32]]) {
        if let Some(_) = inputs[1] {
            self.process_with_modulation(inputs, outputs);
        } else {
            self.process_no_modulation(inputs, outputs);
        }
    }
    fn handle_msg(&mut self, msg: crate::msg::NodeMessage) {
        if let NodeMessage::SetParam(payload) = msg {
            let incoming = match (payload.param_name, payload.value) {
                ("hold_time", RtValue::U32(val)) => val as u32,
                ("hold_time", RtValue::I32(val)) => val as u32,
                ("hold_time", RtValue::F32(val)) => val.round() as u32, // TODO: Semantics?
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

#[cfg(test)]
mod test {
    use crate::{
        config::Config, harness::build_placeholder_context, node::Node,
        nodes::modular::sample_and_hold::SampleAndHold,
    };

    /// Here we have 4 different values, one second apart.
    /// We then sample every half second, and expect to see each one twice.
    ///
    /// As a quick smoke test, input and output should be the same, since we
    /// are sampling twice per block and twice per second.
    #[test]
    fn noop_on_continous_block_with_factorable_held_size() {
        // Test input block, 4 blocks of 256, 256 sample rate
        let inputs: [f32; 1024] = std::array::from_fn(|i| {
            let nth_block_floored = i / 256;
            nth_block_floored as f32
        });

        dbg!(&inputs);

        let mut outputs = [0.0_f32; 1024];

        let config = Config {
            block_size: 256,
            channels: 1,
            rt_capacity: 0,
            sample_rate: 256,
        };

        let mut node = SampleAndHold::new(128, 256.0); // Every half second

        let mut ctx = build_placeholder_context(config);

        for i in 0..4 {
            let lo = i * 256;
            let hi = lo + 256;
            let new_inputs = [Some(&inputs[lo..hi]), None]; // No modulation here
            node.process(&mut ctx, &new_inputs, &mut [&mut outputs[lo..hi]]);
        }

        // Since we held on the constant block and block / 2 boundary, this should have been a no-op
        inputs
            .iter()
            .zip(outputs)
            .enumerate()
            .for_each(|(i, (input, output))| {
                dbg!(i);
                assert_eq!(*input, output);
            });
    }
}
