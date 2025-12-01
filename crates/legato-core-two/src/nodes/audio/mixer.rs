use crate::{
    nodes::{
        Node, NodeInputs,
        ports::{PortBuilder, Ported, Ports},
    },
    runtime::lanes::{LANES, Vf32},
    utils::math::fast_tanh_vf32,
};

// TODO: More mixers, matrix, panning, etc.

/// A simple node for mixing down tracks.
///
/// A "track", in this context, is an arbitrary amount of channels.
///
/// So, this TrackMixer can take say two tracks of stereo -> one track stereo
pub struct TrackMixer {
    chans_per_track: usize,
    ports: Ports,
    gain: Vec<Vf32>,
}

impl TrackMixer {
    pub fn new(chans_per_track: usize, tracks: usize, gain: Vec<f32>) -> Self {
        Self {
            chans_per_track,
            gain: gain.into_iter().map(|x| Vf32::splat(x)).collect(),
            ports: PortBuilder::default()
                .audio_in(chans_per_track * tracks)
                .audio_out(chans_per_track)
                .build(),
        }
    }
}

impl Node for TrackMixer {
    fn process<'a>(
        &mut self,
        ctx: &mut crate::runtime::context::AudioContext,
        ai: &NodeInputs,
        ao: &mut NodeInputs,
        _: &NodeInputs,
        _: &mut NodeInputs,
    ) {
        for (i, track) in ai.chunks_exact(self.chans_per_track).enumerate() {
            let gain = self.gain[i];
            for (chan_idx, chan) in track.into_iter().enumerate() {
                for (chunk_in, chunk_out) in chan
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
}

impl Ported for TrackMixer {
    fn get_ports(&self) -> &Ports {
        &self.ports
    }
}
