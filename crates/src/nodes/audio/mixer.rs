use crate::{
    context::AudioContext,
    math::fast_tanh_vf32,
    node::{Channels, Inputs, Node},
    ports::{PortBuilder, Ports},
    simd::{LANES, Vf32},
};

// TODO: More mixers, matrix, panning, etc.

/// A simple node for mixing down tracks.
///
/// A "track", in this context, is an arbitrary amount of channels.
///
/// So, this TrackMixer can take say two tracks of stereo -> one track stereo
#[derive(Clone)]
pub struct TrackMixer {
    chans_per_track: usize,
    ports: Ports,
    gain: Vec<Vf32>,
}

impl TrackMixer {
    pub fn new(chans_per_track: usize, tracks: usize, gain: Vec<f32>) -> Self {
        Self {
            chans_per_track,
            gain: gain.into_iter().map(Vf32::splat).collect(),
            ports: PortBuilder::default()
                .audio_in(chans_per_track * tracks)
                .audio_out(chans_per_track)
                .build(),
        }
    }
}

impl Node for TrackMixer {
    fn process<'a>(&mut self, _: &mut AudioContext, ai: &Inputs, ao: &mut Channels) {
        // Note: the graph does not explicity clear ao. So, if you are going to do multiple passes, you have to clear it first
        for buffer in ao.iter_mut() {
            buffer.fill(0.0);
        }

        for (i, track) in ai.chunks_exact(self.chans_per_track).enumerate() {
            let gain = self.gain[i];
            for (chan_idx, chan) in track.iter().enumerate() {
                for (chunk_in, chunk_out) in chan
                    .unwrap()
                    .chunks_exact(LANES)
                    .zip(ao[chan_idx].chunks_exact_mut(LANES))
                {
                    let gained = Vf32::from_slice(chunk_in) * gain;
                    let mut out = Vf32::from_slice(chunk_out);
                    out += gained;
                    chunk_out.copy_from_slice(&out.to_array());
                }
            }
        }
        for chan in ao {
            for chunk in chan.chunks_exact_mut(LANES) {
                chunk.copy_from_slice(fast_tanh_vf32(Vf32::from_slice(chunk)).as_array());
            }
        }
    }
    fn ports(&self) -> &Ports {
        &self.ports
    }
}

/// A mono -> N mixer with unity gain
#[derive(Clone)]
pub struct MonoFanOut {
    ports: Ports,
}

impl MonoFanOut {
    pub fn new(chans_out: usize) -> Self {
        Self {
            ports: PortBuilder::default()
                .audio_in(1)
                .audio_out(chans_out)
                .build(),
        }
    }
}

impl Node for MonoFanOut {
    fn process(&mut self, _: &mut AudioContext, ai: &Inputs, ao: &mut Channels) {
        // TODO: Chunks + SIMD
        let chans_out = self.ports.audio_out.len();
        let gain = 1.0 / f32::sqrt(chans_out as f32);

        for (i, sample) in ai[0].unwrap().iter().enumerate() {
            let normalized = sample * gain;
            for chan_out in ao.iter_mut() {
                chan_out[i] = normalized
            }
        }
    }
    fn ports(&self) -> &Ports {
        &self.ports
    }
}
