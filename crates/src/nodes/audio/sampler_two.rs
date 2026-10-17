use crate::{
    node::Node,
    ports::{PortBuilder, Ports},
    resources::ExternalBufferKey,
};

/// A more creative sampler implementation with fractional playheads,
/// "pitch" based playhead speed, audio-rate bounds, etc.
///
/// Using cubic-hermite interpolation here for the time being. Perhaps some
/// developers may want an adjustable rate here for platforms with less compute?
///
/// Repitching here will change the sound of the sample as we are not using
/// any clever math or implementations for repitching other than changing
/// the playhead speed.
///
/// Higher quality, more expensive approaches will be under consideration
/// in the future.
///
/// For the time being, if you seek more natural repitching, the [`Grain`]
/// node with a more hann-esque shape acts as a reasonabl instrument that
/// can repitch. This is however a naive OLA algorithm and not suitable say
/// for DJing, mixing and mastering.
pub struct SamplerTwo {
    /// The active bounds of the selected sample, used to change the start and end of the sample.
    /// This cannot be larger than the selected sample
    bounds: (usize, usize),
    /// A fractional speed for adjusting the playhead
    speed: f32,
    /// A fractional playhead, as we can interpolate between samples
    playhead: f32,
    /// Used to load shared resources, in this case an [`ExternalBuffer`].
    /// This is an Arc<[f32]> loaded in by another thread.
    ///
    /// TODO: Audit who owns the drop, probably not realtime safe!
    sample_key: ExternalBufferKey,
    ports: Ports,
}

impl SamplerTwo {
    pub fn new(sample_key: ExternalBufferKey, start: usize, end: usize, chans: usize) -> Self {
        Self {
            sample_key,
            playhead: 0.0,
            bounds: (start, end),
            ports: PortBuilder::default()
                .control_in_named(&["trig", "start", "end"])
                .audio_out(chans)
                .build(),
        }
    }
}

impl Node for SamplerTwo {
    fn process(
        &mut self,
        ctx: &mut crate::prelude::AudioContext,
        inputs: &crate::prelude::Inputs,
        outputs: &mut [&mut [f32]],
    ) {
        let trig_chan = inputs[0];
        let start_chan = inputs[1];
        let end_chan = inputs[1];

        if let Some(buffer) = ctx.get_resources().get_external_buffer(self.sample_key) {
            for (c, chan_out) in outputs.iter_mut().enumerate() {
                let chan = buffer.channel(c);

                // TODO: this feels a bit hot...consider hoisting certain branches
                for (i, sample) in chan_out.iter_mut().enumerate() {
                    let start = start_chan.map_or(self.bounds.0.min(buffer.len_samples()), |x| {
                        x[i].floor() as usize
                    });
                    let end = end_chan.map_or(self.bounds.1.max(1), |x| x[i].floor() as usize);

                    let trig = trig_chan.map_or(false, |x| x[i] == 1.0);

                    if trig {
                        self.playhead = start as f32;
                    }

                    *sample = chan[i];
                }
            }
        }
    }
    fn handle_msg(&mut self, _msg: crate::msg::NodeMessage) {}
    fn ports(&self) -> &Ports {
        &self.ports
    }
}
