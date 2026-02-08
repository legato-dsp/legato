use crate::{
    context::AudioContext,
    msg::{self, RtValue},
    node::{Inputs, Node},
    ports::{PortBuilder, Ports},
    simd::LANES,
};

#[derive(Clone, Debug)]
pub struct Phasor {
    phase: f32,
    freq: f32,
    ports: Ports,
}

impl Phasor {
    pub fn new(freq: f32) -> Self {
        Self {
            phase: 0.0,
            freq,
            ports: PortBuilder::default().audio_out(1).build(),
        }
    }
    #[inline(always)]
    fn tick(&mut self, inc: f32) -> f32 {
        let mut p = self.phase + inc;

        if p >= 1.0 {
            p -= 1.0;
        }

        self.phase = p;
        p * 2.0 - 1.0
    }
}

impl Node for Phasor {
    fn process(&mut self, ctx: &mut AudioContext, _: &Inputs, outputs: &mut [&mut [f32]]) {
        let fs_recipricol = 1.0 / ctx.get_config().sample_rate as f32;
        let inc = self.freq * fs_recipricol;

        outputs
            .get_mut(0)
            .unwrap()
            .iter_mut()
            .for_each(|x| *x = self.tick(inc));
    }

    fn ports(&self) -> &Ports {
        &self.ports
    }

    fn handle_msg(&mut self, msg: crate::msg::NodeMessage) {
        match msg {
            msg::NodeMessage::SetParam(inner) => match (inner.param_name, inner.value) {
                ("freq", RtValue::F32(val)) => self.freq = val,
                ("freq", RtValue::U32(val)) => self.freq = val as f32,
                _ => (),
            },
            _ => (),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use crate::harness::{get_node_test_harness_stereo, get_node_test_harness_stereo_4096};

    fn run_phasor(freq: f32, blocks: usize) -> Vec<f32> {
        let mut graph = get_node_test_harness_stereo_4096(Box::new(Phasor::new(freq)));

        let mut out = Vec::new();

        for _ in 0..blocks {
            let block = graph.next_block(None);

            let ch0 = &block[0];

            out.extend_from_slice(ch0);
        }

        out
    }

    fn approx(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() < eps
    }

    #[test]
    fn phasor_output_range() {
        let out = run_phasor(440.0, 4);

        for &x in &out {
            assert!(x >= -1.0 && x <= 1.0, "Out of range: {}", x);
        }
    }

    #[test]
    fn phasor_zero_freq() {
        let out = run_phasor(0.0, 2);

        for &x in &out {
            assert!(approx(x, -1.0, 1e-6));
        }
    }

    #[test]
    fn phasor_one_hz_cycle() {
        // here we use a more easily divisible block and sample rate to ensure
        let sr = 48_000;
        let block_size = 4_000;
        let blocks = sr / block_size;

        let mut graph = get_node_test_harness_stereo(Box::new(Phasor::new(1.0)), sr, block_size);

        let mut out = Vec::new();
        for _ in 0..blocks {
            let block = graph.next_block(None);
            out.extend_from_slice(&block[0]);
        }

        let first = out[0];
        let last = out[sr as usize - 1];

        let diff = (last - first).abs();
        assert!(diff < 2e-3, "Cycle not complete: diff = {}", diff); // TODO: Evaluate acceptible difference here
    }

    /// Phase is continuous across blocks
    #[test]
    fn phasor_block_continuity() {
        let mut graph = get_node_test_harness_stereo_4096(Box::new(Phasor::new(440.0)));

        // First block
        let a = graph.next_block(None);
        let last_a = a[0][4095];

        // Second block
        let b = graph.next_block(None);
        let first_b = b[0][0];

        let expected_step = (440.0 / 48_000.0) * 2.0;

        let diff = first_b - last_a;

        assert!(
            approx(diff, expected_step, 1e-3),
            "Discontinuity: {} vs {}",
            diff,
            expected_step
        );
    }

    #[test]
    fn phasor_long_run_stable() {
        let mut graph = get_node_test_harness_stereo_4096(Box::new(Phasor::new(1234.5)));

        for _ in 0..100 {
            let block = graph.next_block(None);

            for ch in block.iter() {
                for &x in ch.iter() {
                    assert!(x.is_finite());
                    assert!(x >= -1.1 && x <= 1.1);
                }
            }
        }
    }
}
