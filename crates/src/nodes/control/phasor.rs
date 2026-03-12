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
        let mut runtime = get_node_test_harness_stereo_4096(Box::new(Phasor::new(freq)));
        let block_size = runtime.get_config().block_size;
        let mut out = Vec::with_capacity(blocks * block_size);

        for _ in 0..blocks {
            let view = runtime.next_block(None);
            // In the new graph, we access the slice from the channels array
            out.extend_from_slice(view.channels[0]);
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
            // Phasor at 0Hz should stay at its initial phase (mapped to -1.0)
            assert!(approx(x, -1.0, 1e-6), "Expected -1.0, got {}", x);
        }
    }

    #[test]
    fn phasor_one_hz_cycle() {
        let sr = 48_000;
        let block_size = 4_000;
        let mut runtime = get_node_test_harness_stereo(Box::new(Phasor::new(1.0)), sr, block_size);

        let mut out = Vec::new();
        for _ in 0..(sr / block_size) {
            let view = runtime.next_block(None);
            out.extend_from_slice(view.channels[0]);
        }

        let first = out[0];
        let last = out[sr - 1];

        // A 1Hz phasor over 1s should end nearly where it started
        let diff = (last - first).abs();
        assert!(diff < 2e-3, "Cycle not complete: diff = {}", diff);
    }

    #[test]
    fn phasor_block_continuity() {
        let freq = 440.0;
        let sr = 48_000;
        let mut runtime = get_node_test_harness_stereo_4096(Box::new(Phasor::new(freq)));

        let block_size = runtime.get_config().block_size;

        let view_a = runtime.next_block(None);
        let last_a = view_a.channels[0][block_size - 1];

        let view_b = runtime.next_block(None);
        let first_b = view_b.channels[0][0];

        // The phase increment per sample, mapped to the -1.0 to 1.0 range (range of 2.0)
        let expected_step = (freq / sr as f32) * 2.0;
        let actual_diff = first_b - last_a;

        assert!(
            approx(actual_diff, expected_step, 1e-3),
            "Discontinuity at block boundary: expected step {}, got {}",
            expected_step,
            actual_diff
        );
    }

    #[test]
    fn phasor_long_run_stable() {
        let mut runtime = get_node_test_harness_stereo_4096(Box::new(Phasor::new(1234.5)));

        for _ in 0..100 {
            let view = runtime.next_block(None);
            for i in 0..view.chans {
                for &x in view.channels[i] {
                    assert!(x.is_finite(), "Sample is NaN or Inf");
                    assert!(x >= -1.1 && x <= 1.1, "Sample exploded: {}", x);
                }
            }
        }
    }
}
