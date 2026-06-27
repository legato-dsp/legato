use crate::{context::AudioContext, msg::NodeMessage, node::Node, ports::Ports};

const MAX_FRAME_PORTS: usize = 64;

/// A node that processes one sample-frame at a time.
///
/// In the future, this might be used for subgraphs that require feedback.
/// For now, it's useful to block adapt per sample nodes to the block rate.
///
/// You would need this if you had for instance a tight FDN in a reverb,
/// per sample FM synth modulation, etc.
pub trait PerSampleNode: Send {
    fn ports(&self) -> &Ports;
    // Each item in each frame represents ONE sample in a channel
    fn tick(&mut self, in_frame: &[f32], out_frame: &mut [f32]);
    fn handle_msg(&mut self, _msg: NodeMessage) {}
}

/// Drives a [`SampleNode`] as a block-rate [`Node`], owning the reusable frame
/// scratch so the hot path is allocation-free.
pub struct PerSample<T: PerSampleNode> {
    inner: T,
    in_frame: Box<[f32]>,
    out_frame: Box<[f32]>,
}

impl<T: PerSampleNode> PerSample<T> {
    pub fn new(inner: T) -> Self {
        let ports = inner.ports();
        let n_in = ports.audio_in.len();
        let n_out = ports.audio_out.len();
        assert!(
            n_in <= MAX_FRAME_PORTS && n_out <= MAX_FRAME_PORTS,
            "PerSample supports up to {MAX_FRAME_PORTS} ports per side (got {n_in} in, {n_out} out)"
        );

        Self {
            in_frame: vec![0.0; n_in].into_boxed_slice(),
            out_frame: vec![0.0; n_out].into_boxed_slice(),
            inner,
        }
    }

    pub fn inner(&self) -> &T {
        &self.inner
    }

    pub fn into_inner(self) -> T {
        self.inner
    }
}

impl<T: PerSampleNode + Clone> Clone for PerSample<T> {
    fn clone(&self) -> Self {
        Self::new(self.inner.clone())
    }
}

impl<T: PerSampleNode + Clone + 'static> Node for PerSample<T> {
    fn process(
        &mut self,
        _ctx: &mut AudioContext,
        inputs: &crate::node::Inputs,
        outputs: &mut [&mut [f32]],
    ) {
        let n_in = self.in_frame.len();
        let n_out = self.out_frame.len();

        // Hoist per-port input slices out of the sample loop.
        let mut ins: [Option<&[f32]>; MAX_FRAME_PORTS] = [None; MAX_FRAME_PORTS];
        for i in 0..n_in {
            ins[i] = inputs.get(i).and_then(|x| *x);
        }

        let block = outputs.first().map_or(0, |o| o.len());

        for s in 0..block {
            for i in 0..n_in {
                self.in_frame[i] = ins[i].map_or(0.0, |b| b[s]);
            }

            self.inner.tick(&self.in_frame, &mut self.out_frame);

            for j in 0..n_out {
                outputs[j][s] = self.out_frame[j];
            }
        }
    }

    fn ports(&self) -> &Ports {
        self.inner.ports()
    }

    fn handle_msg(&mut self, msg: NodeMessage) {
        self.inner.handle_msg(msg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{BlockSize, Config},
        harness::build_placeholder_context,
        node::Node,
        ports::PortBuilder,
    };

    #[derive(Clone)]
    struct Lp {
        a: f32,
        state: f32,
        ports: Ports,
    }

    impl PerSampleNode for Lp {
        fn ports(&self) -> &Ports {
            &self.ports
        }
        fn tick(&mut self, inp: &[f32], out: &mut [f32]) {
            self.state = (1.0 - self.a) * inp[0] + self.a * self.state;
            out[0] = self.state;
        }
    }

    fn lp(a: f32) -> PerSample<Lp> {
        PerSample::new(Lp {
            a,
            state: 0.0,
            ports: PortBuilder::default().audio_in(1).audio_out(1).build(),
        })
    }

    /// Same node must produce the same samples at any block size.
    #[test]
    fn block_size_invariant() {
        let input: Vec<f32> = (0..256).map(|i| (i as f32).sin()).collect();

        let mut big = lp(0.5);
        let mut big_ctx = build_placeholder_context(Config::new(48_000, BlockSize::Block256, 1, 0));
        let mut big_out = vec![0.0f32; 256];
        {
            let inputs = [Some(input.as_slice())];
            let mut outs = [big_out.as_mut_slice()];
            big.process(&mut big_ctx, &inputs, &mut outs);
        }

        let mut small = lp(0.5);
        let mut ctx = build_placeholder_context(Config::new(48_000, BlockSize::Block64, 1, 0));
        let mut small_out = vec![0.0f32; 256];
        for blk in 0..4 {
            let range = blk * 64..blk * 64 + 64;
            let inputs = [Some(&input[range.clone()])];
            let mut outs = [&mut small_out[range]];
            small.process(&mut ctx, &inputs, &mut outs);
        }

        for (a, b) in big_out.iter().zip(small_out.iter()) {
            assert!(
                (a - b).abs() < 1e-6,
                "block size changed the output: {a} vs {b}"
            );
        }
    }

    #[test]
    fn unpatched_input_is_zero() {
        let mut node = lp(0.0);
        let mut ctx = build_placeholder_context(Config::new(48_000, BlockSize::Block64, 1, 0));

        let inputs: [Option<&[f32]>; 1] = [None];
        let mut out = vec![1.0f32; 64];
        {
            let mut outs = [out.as_mut_slice()];
            node.process(&mut ctx, &inputs, &mut outs);
        }
        assert!(out.iter().all(|&x| x == 0.0));
    }
}
