use crate::{LegatoApp, resources::AudioInputKey};

/// Runs a fixed block size graph under a host that calls with arbitrary block sizes,
/// e.g. a plugin `process` callback.
///
/// This will incur some latency, so the adapter adds
/// [`BlockAdapter::latency_samples`] of latency.
///
/// This can then reported back to the host for delay compensation (common in VSTs)
pub struct BlockAdapter {
    block_size: usize,
    chans: usize,
    output: Box<[f32]>, // chans * block_size
    pos: usize,
}

impl BlockAdapter {
    pub fn new(app: &LegatoApp) -> Self {
        let config = app.get_config();
        Self {
            block_size: config.block_size,
            chans: config.channels,
            output: vec![0.0; config.channels * config.block_size].into(),
            pos: 0,
        }
    }

    pub fn latency_samples(&self) -> usize {
        self.block_size
    }

    pub fn reset(&mut self) {
        self.output.fill(0.0);
        self.pos = 0;
    }

    /// Process `io` in place: it is read as the `main` input (if given), then overwritten
    /// with graph output. `aux` inputs, such as sidechains, must be `io`'s length.
    pub fn process(
        &mut self,
        app: &mut LegatoApp,
        main: Option<AudioInputKey>,
        io: &mut [&mut [f32]],
        aux: &[(AudioInputKey, &[&[f32]])],
    ) {
        let frames = io.first().map_or(0, |c| c.len());
        let mut done = 0;

        while done < frames {
            let n = (self.block_size - self.pos).min(frames - done);
            let range = done..done + n;

            // Map the specific inputs to their corresponding keys
            // This allows us to use all the inputs as graph nodes

            if let Some(main_key) = main {
                for (c, chan) in io.iter().enumerate() {
                    app.write_audio_input(main_key, c, self.pos, &chan[range.clone()]);
                }
            }

            for (aux_key, chans) in aux {
                for (c, chan) in chans.iter().enumerate() {
                    app.write_audio_input(*aux_key, c, self.pos, &chan[range.clone()]);
                }
            }

            for (c, chan) in io.iter_mut().enumerate() {
                let destination = &mut chan[range.clone()];
                if c < self.chans {
                    let start = c * self.block_size + self.pos;
                    destination.copy_from_slice(&self.output[start..start + n]);
                } else {
                    destination.fill(0.0);
                }
            }

            self.pos += n;
            done += n;

            if self.pos == self.block_size {
                self.pos = 0;

                let view = app.next_block();
                for (c, out) in self.output.chunks_exact_mut(self.block_size).enumerate() {
                    match view.channels[..view.chans].get(c) {
                        Some(chan) => out.copy_from_slice(chan),
                        None => out.fill(0.0),
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        builder::{LegatoBuilder, Unconfigured},
        config::Config,
    };
    use proptest::prelude::*;

    const BLOCK_SIZE: usize = 64;

    fn passthrough(input: &str) -> LegatoApp {
        let config = Config {
            sample_rate: 48_000,
            block_size: BLOCK_SIZE,
            channels: 2,
            rt_capacity: 0,
        };
        let src = format!(
            r#"
            audio {{
                external {{ interface_name: "{input}", chans: 2 }},
            }}

            {{ external }}
            "#
        );
        let (app, _) = LegatoBuilder::<Unconfigured>::new(config)
            .register_host_audio_input("main", 2)
            .register_host_audio_input("sidechain", 2)
            .build_dsl(&src)
            .expect("graph should build");
        app
    }

    fn signal(chan: usize, len: usize) -> Vec<f32> {
        (0..len).map(|i| (chan * 100_000 + i + 1) as f32).collect()
    }

    fn delayed(src: &[f32], by: usize) -> Vec<f32> {
        let mut out = vec![0.0; by.min(src.len())];
        out.extend_from_slice(&src[..src.len().saturating_sub(by)]);
        out
    }

    fn render(input: &str, host_blocks: &[usize]) -> (Vec<Vec<f32>>, Vec<Vec<f32>>, usize) {
        let mut app = passthrough(input);
        let mut adapter = BlockAdapter::new(&app);
        let key = app.audio_input_key(input).unwrap();
        let total: usize = host_blocks.iter().sum();
        let inputs = [signal(0, total), signal(1, total)];
        let mut outputs = vec![Vec::new(), Vec::new()];

        let mut at = 0;
        for &n in host_blocks {
            let mut l = inputs[0][at..at + n].to_vec();
            let mut r = inputs[1][at..at + n].to_vec();
            {
                let mut io = [l.as_mut_slice(), r.as_mut_slice()];
                if input == "main" {
                    adapter.process(&mut app, Some(key), &mut io, &[]);
                } else {
                    let sc = [&inputs[0][at..at + n], &inputs[1][at..at + n]];
                    io.iter_mut().for_each(|c| c.fill(-1.0));
                    adapter.process(&mut app, None, &mut io, &[(key, &sc)]);
                }
            }
            outputs[0].extend(l);
            outputs[1].extend(r);
            at += n;
        }
        (inputs.to_vec(), outputs, adapter.latency_samples())
    }

    proptest! {
        #[test]
        fn output_is_input_delayed_by_one_block(
            host_blocks in prop::collection::vec(1usize..300, 1..40),
            sidechain in any::<bool>(),
        ) {
            let input = if sidechain { "sidechain" } else { "main" };
            let (inputs, outputs, latency) = render(input, &host_blocks);
            prop_assert_eq!(latency, BLOCK_SIZE);
            for (i, o) in inputs.iter().zip(&outputs) {
                prop_assert_eq!(o, &delayed(i, latency));
            }
        }
    }

    #[test]
    fn reset_clears_pending_output() {
        let mut app = passthrough("main");
        let mut adapter = BlockAdapter::new(&app);
        let key = app.audio_input_key("main");
        let mut l = vec![1.0; BLOCK_SIZE + 10];
        let mut r = l.clone();
        adapter.process(&mut app, key, &mut [&mut l, &mut r], &[]);

        adapter.reset();
        let mut l = vec![0.0; BLOCK_SIZE];
        let mut r = l.clone();
        adapter.process(&mut app, key, &mut [&mut l, &mut r], &[]);
        assert!(l.iter().chain(&r).all(|&s| s == 0.0));
    }
}
