use crate::{
    nodes::{
        Node, NodeInputs, ports::{PortBuilder, Ported, Ports}
    },
    runtime::context::AudioContext,
};

pub struct Sine {
    freq: f32,
    phase: f32,
    ports: Ports,
}

impl Sine {
    pub fn new(freq: f32, chans: usize) -> Self {
        Self {
            freq,
            phase: 0.0,
            ports: PortBuilder::default()
                .audio_in_named(&["fm"])
                .audio_out(chans)
                .build(),
        }
    }
}

impl Node for Sine {
    fn process(
        &mut self,
        ctx: &mut AudioContext,
        ai: &NodeInputs,
        ao: &mut NodeInputs,
        _: &NodeInputs,
        _: &mut NodeInputs,
    ) {
        let config = ctx.get_config();
        let fs = config.sample_rate as f32;

        let fm_in = &ai[0];

        for n in 0..config.audio_block_size {
            let mod_amt = fm_in[n];

            let freq = self.freq + mod_amt;

            self.phase += freq / fs;
            self.phase = self.phase.fract();

            let sample = (self.phase * std::f32::consts::TAU).sin();

            for chan in ao.iter_mut() {
                chan[n] = sample;
            }
        }
    }
}

impl Ported for Sine {
    fn get_ports(&self) -> &Ports {
        &self.ports
    }
}
