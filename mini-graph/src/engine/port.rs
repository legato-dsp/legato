use crate::engine::buffer::{zero_frame, Frame};


enum PortBehavior {
    Default, // Take the first sample
    Sum,
    SumNormalized,
} 

struct Port {
    name: &'static str,
    in_behavior: PortBehavior,
}

#[inline(always)]
pub fn handle_input<const N: usize, const C: usize>(inputs: &[Frame<N, C>], output: &mut Frame<N, C>, behavior: PortBehavior) {
    match behavior {
        PortBehavior::Default => {
            if let Some(input) = inputs.first(){
                for c in 0..N {
                    for n in 0..C {
                        output[c][n] = input[c][n];
                    }
                }
            }
            else {
                zero_frame(output);
            }
        },
        PortBehavior::Sum => {
            zero_and_sum_frame(&inputs, output);
        },
        PortBehavior::SumNormalized => {
            zero_and_sum_frame(&inputs, output);

            let denom = inputs.len().max(1) as f32;
            for c in 0..C {
                for n in 0..N {
                    output[c][n] /= denom;
                }
            }
        }
    }
}

#[inline(always)]
fn zero_and_sum_frame<const N: usize, const C: usize>(frames: &[Frame<N, C>], output: &mut Frame<N, C>) {
        zero_frame(output);
        for frame in frames {
            for c in 0..C {
                for n in 0..N {
                    output[c][n] += frame[c][n];
            }
        }
    }
}



#[cfg(test)]
mod tests {
    use crate::engine::buffer::Buffer;

    use super::*;

    // Build const frames
    fn const_frame<const N: usize, const C: usize>(vals: [f32; C]) -> Frame<N, C> {
        let mut f: Frame<N, C> = [Buffer::<N>::SILENT; C];
        for c in 0..C {
            let mut buf = Buffer::<N>::from([0.0; N]);
            for n in 0..N {
                buf[n] = vals[c];
            }
            f[c] = buf;
        }
        f
    }

    fn assert_frame_eq<const N: usize, const C: usize>(frame: &Frame<N, C>, expected: [f32; C]) {
        for c in 0..C {
            for n in 0..N {
                assert!(
                    (frame[c][n] - expected[c]).abs() < 1e-6,
                    "Failed: channel {c}, sample {n}: got {}, expected {}",
                    frame[c][n],
                    expected[c]
                );
            }
        }
    }

    #[test]
    fn default_behavior_takes_first_input() {
        const N: usize = 2;
        const C: usize = 2;

        let a = const_frame::<N, C>([0.25, -0.25]);
        let b = const_frame::<N, C>([0.75, 0.75]);

        let mut out: Frame<N, C> = [Buffer::<N>::SILENT; C];
        handle_input::<N, C>(&[a, b], &mut out, PortBehavior::Default);
        assert_frame_eq(&out, [0.25, -0.25]);
    }

    #[test]
    fn default_behavior_silence_when_no_inputs() {
        const N: usize = 2;
        const C: usize = 2;

        let mut out: Frame<N, C> = [Buffer::<N>::from([1.0; N]); C]; // This should be "silenced"
        handle_input::<N, C>(&[], &mut out, PortBehavior::Default);
        assert_frame_eq(&out, [0.0, 0.0]);
    }

    #[test]
    fn sum_behavior_adds_inputs() {
        const N: usize = 4;
        const C: usize = 2;

        let a = const_frame::<N, C>([0.1, 0.2]);
        let b = const_frame::<N, C>([0.3, -0.4]);
        let c = const_frame::<N, C>([0.6, 0.7]);

        let mut out: Frame<N, C> = [Buffer::<N>::SILENT; C];
        handle_input::<N, C>(&[a, b, c], &mut out, PortBehavior::Sum);

        assert_frame_eq(&out, [1.0, 0.5]);
    }

    #[test]
    fn sum_normalized_behavior_averages_inputs() {
        const N: usize = 32;
        const C: usize = 2;

        let a = const_frame::<N, C>([3.0, -1.0]);
        let b = const_frame::<N, C>([2.0, 0.5]);
        let c = const_frame::<N, C>([4.0, 0.5]);

        let mut out: Frame<N, C> = [Buffer::<N>::from([42.0; N]); C];
        handle_input::<N, C>(&[a, b, c], &mut out, PortBehavior::SumNormalized);
        assert_frame_eq(&out, [3.0, 0.0]);
    }

    #[test]
    fn sum_normalized_with_no_inputs_yields_silence() {
        const N: usize = 2;
        const C: usize = 2;

        let mut out: Frame<N, C> = [Buffer::<N>::from([5.0; N]); C];
        handle_input::<N, C>(&[], &mut out, PortBehavior::SumNormalized);
        assert_frame_eq(&out, [0.0, 0.0]);
    }
}