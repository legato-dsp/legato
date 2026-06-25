use std::time::Duration;

use crate::{
    context::AudioContext,
    node::{Inputs, Node},
    ports::{PortBuilder, Ports},
    resources::DelayLineKey,
    simd::{LANES, Vf32},
};

#[derive(Clone)]
pub struct DelayWrite {
    delay_line_keys: Vec<DelayLineKey>,
    ports: Ports,
}
impl DelayWrite {
    pub fn new(delay_line_keys: Vec<DelayLineKey>, chans: usize) -> Self {
        Self {
            delay_line_keys,
            ports: PortBuilder::default()
                .audio_in(chans)
                .audio_out(chans) // Just for graph semantics
                .build(),
        }
    }
}

impl Node for DelayWrite {
    fn process(&mut self, ctx: &mut AudioContext, ai: &Inputs, _: &mut [&mut [f32]]) {
        let resources = ctx.get_resources_mut();

        for (c, chan_opt) in ai.iter().enumerate() {
            if let Some(chan) = chan_opt {
                let mut view = resources.delay_line_view_mut(self.delay_line_keys[c]);
                for &sample in chan.iter() {
                    view.push(sample);
                }
            }
        }
    }
    fn ports(&self) -> &Ports {
        &self.ports
    }
}

/// Interpolation quality used when reading from the delay line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DelayQuality {
    /// Cheaper Linear Interpolation
    Linear,
    /// More Expensive Cubic Hermite Interpolation
    #[default]
    Cubic,
}

#[derive(Clone)]
pub struct DelayRead {
    delay_line_keys: Vec<DelayLineKey>,
    len: Duration, // Different times for each channel if desired
    quality: DelayQuality,
    ports: Ports,
}
impl DelayRead {
    pub fn new(
        chans: usize,
        delay_line_keys: Vec<DelayLineKey>,
        len: Duration,
        quality: DelayQuality,
    ) -> Self {
        Self {
            delay_line_keys,
            len,
            quality,
            ports: PortBuilder::default().audio_out(chans).build(),
        }
    }

    fn process_linear(&mut self, ctx: &mut AudioContext, ao: &mut [&mut [f32]]) {
        let config = ctx.get_config();
        let block_size = config.block_size;
        let sr = config.sample_rate as f32;

        let resources = ctx.get_resources();

        for (c, chan) in ao.iter_mut().enumerate() {
            let delay_time = self.len.as_secs_f32();
            let view = resources.delay_line_view(self.delay_line_keys[c]);

            let offsets = |cidx: usize| -> Vf32 {
                let chunk_start = LANES * cidx;
                Vf32::from_array(std::array::from_fn(|lane| {
                    delay_time * sr + (block_size - (chunk_start + lane)) as f32
                }))
            };

            for (cidx, chunk) in chan.chunks_exact_mut(LANES).enumerate() {
                let out = view.read_linear_simd(offsets(cidx));
                chunk.copy_from_slice(out.as_array());
            }
        }
    }
    fn process_cubic(&mut self, ctx: &mut AudioContext, ao: &mut [&mut [f32]]) {
        let config = ctx.get_config();
        let block_size = config.block_size;
        let sr = config.sample_rate as f32;

        let resources = ctx.get_resources();

        for (c, chan) in ao.iter_mut().enumerate() {
            let delay_time = self.len.as_secs_f32();
            let view = resources.delay_line_view(self.delay_line_keys[c]);

            let offsets = |cidx: usize| -> Vf32 {
                let chunk_start = LANES * cidx;
                Vf32::from_array(std::array::from_fn(|lane| {
                    delay_time * sr + (block_size - (chunk_start + lane)) as f32
                }))
            };

            for (cidx, chunk) in chan.chunks_exact_mut(LANES).enumerate() {
                let out = view.read_cubic_simd(offsets(cidx));
                chunk.copy_from_slice(out.as_array());
            }
        }
    }
}

impl Node for DelayRead {
    fn process(&mut self, ctx: &mut AudioContext, _: &Inputs, ao: &mut [&mut [f32]]) {
        match self.quality {
            DelayQuality::Linear => self.process_linear(ctx, ao),
            DelayQuality::Cubic => self.process_cubic(ctx, ao),
        }
    }
    fn ports(&self) -> &Ports {
        &self.ports
    }
}

#[cfg(test)]
mod test_delay_simd_equivalence {
    use crate::ring::RingBuffer;

    use super::*;
    use rand::Rng;

    #[test]
    fn scalar_and_simd_writes_match() {
        const CHANS: usize = 1;
        const CAP: usize = 2048;
        const BLOCK: usize = 4096;

        let mut rb_scalar = RingBuffer::new(CAP);
        let mut rb_simd = RingBuffer::new(CAP);

        let mut inputs_raw = [[0.0; BLOCK]; CHANS];

        let mut rng = rand::rng();

        for s in &mut inputs_raw[0] {
            *s = rng.random::<f32>();
        }

        let buf = &inputs_raw[0];

        for n in 0..BLOCK {
            rb_scalar.push(buf[n]);
        }

        for chunk in buf.iter().as_slice().chunks(LANES) {
            rb_simd.push_simd(&Vf32::from_slice(chunk));
        }

        let data_scalar = rb_scalar.get_data();
        let data_simd = rb_simd.get_data();

        for i in 0..CAP {
            let a = data_scalar[i];
            let b = data_simd[i];
            assert!(
                (a - b).abs() < 1e-10,
                "data mismatch at index {}: scalar={}, simd={}",
                i,
                a,
                b
            );
        }
    }
}

use crate::{
    builder::{ResourceBuilderView, ValidationError},
    dsl::ir::DSLParams,
    node::DynNode,
    spec::NodeDefinition,
};

impl NodeDefinition for DelayWrite {
    const NAME: &'static str = "delay_write";
    const DESCRIPTION: &'static str = "Writes audio into a named shared delay line";
    const REQUIRED_PARAMS: &'static [&'static str] = &["delay_name"];
    const OPTIONAL_PARAMS: &'static [&'static str] = &["delay_length", "chans"];

    fn create(
        rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        let name = p
            .get_str("delay_name")
            .expect("Could not find required parameter delay_name");

        let len = p
            .get_duration_ms("delay_length")
            .unwrap_or(Duration::from_secs(1));

        let chans = p.get_usize("chans").unwrap_or(2);

        let sr = rb.get_config().sample_rate as f32;
        let capacity = sr * len.as_secs_f32();

        let keys: Vec<_> = if let Some(existing_keys) = rb.get_delay_line_key(&name) {
            existing_keys.iter().for_each(|key| {
                rb.replace_delay_line(*key, (capacity as usize).next_power_of_two());
            });
            existing_keys
        } else {
            (0..chans)
                .map(|_| rb.add_delay_line(&name, (capacity as usize).next_power_of_two()))
                .collect()
        };

        Ok(Box::new(Self::new(keys, chans)))
    }
}

impl NodeDefinition for DelayRead {
    const NAME: &'static str = "delay_read";
    const DESCRIPTION: &'static str =
        "Reads audio from a named shared delay line with interpolation";
    const REQUIRED_PARAMS: &'static [&'static str] = &["delay_name"];
    const OPTIONAL_PARAMS: &'static [&'static str] = &["delay_length", "chans", "quality"];

    fn create(
        rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        let name = p
            .get_str("delay_name")
            .expect("Could not find required parameter delay_name");

        let chans = p.get_usize("chans").unwrap_or(2);

        let delay_len = p
            .get_duration_ms("delay_length")
            .unwrap_or(Duration::from_secs(1));

        let quality = p
            .get_str("quality")
            .map_or(DelayQuality::default(), |q| match q.as_str() {
                "linear" => DelayQuality::Linear,
                "cubic" => DelayQuality::Cubic,
                _ => panic!("Unknown delay quality '{q}', expected 'linear' or 'cubic'"),
            });

        let key = rb
            .get_delay_line_key(&name)
            .unwrap_or_else(|| (0..chans).map(|_| rb.add_delay_line(&name, 1024)).collect());

        Ok(Box::new(Self::new(chans, key, delay_len, quality)))
    }
}
