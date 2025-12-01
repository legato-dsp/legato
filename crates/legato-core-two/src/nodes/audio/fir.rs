use crate::{
    nodes::{
        Node, NodeInputs,
        ports::{PortBuilder, Ported, Ports},
    },
    runtime::context::AudioContext,
    utils::ringbuffer::RingBuffer,
};

pub struct FirFilter {
    coeffs: Vec<f32>,
    state: Vec<RingBuffer>,
    ports: Ports,
}

impl FirFilter {
    pub fn new(coeffs: Vec<f32>, chans: usize) -> Self {
        let coef_size = coeffs.len();
        Self {
            coeffs,
            state: vec![RingBuffer::new(coef_size); chans],
            ports: PortBuilder::default()
                .audio_in(chans)
                .audio_out(chans)
                .build(),
        }
    }
}

impl Node for FirFilter {
    fn process(
        &mut self,
        ctx: &mut AudioContext,
        ai: &NodeInputs,
        ao: &mut NodeInputs,
        ci: &NodeInputs,
        co: &mut NodeInputs,
    ) {
        for ((input, out), state) in ai.iter().zip(ao.iter_mut()).zip(self.state.iter_mut()) {
            for (n, x) in input.iter().enumerate() {
                state.push(*x);
                let mut y = 0.0;
                for (k, &h) in self.coeffs.iter().enumerate() {
                    y += h * state.get_offset(k);
                }
                out[n] = y;
            }
        }
    }
}

impl Ported for FirFilter {
    fn get_ports(&self) -> &Ports {
        &self.ports
    }
}
