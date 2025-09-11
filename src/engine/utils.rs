use std::{cmp::min, usize};

use crate::engine::buffer::Buffer;

// Keeping L, H, and R for DX and potential auto-vectorization wins
#[inline(always)]
pub fn upsample_zoh<const L: usize, const H: usize>(inputs: &Buffer<L>, outputs: &mut Buffer<H>){
    debug_assert!(L < H);  // Assert that we are actually upsampling
    debug_assert!(H % L == 0);

    let r: usize = H / L;

    for n in 0..L {
        let sample = inputs[n];
        for m in 0..r {
            outputs[(n * r) + m] = sample;
        }
    }
}

// Keeping L, H, and R for DX and potential auto-vectorization wins
#[inline(always)]
pub fn upsample_linear<const L: usize, const H: usize>(inputs: &Buffer<L>, outputs: &mut Buffer<H>) {
    debug_assert!(L < H);
    debug_assert!(H % L == 0);

    for i in 0..H {
        let pos: f32 = i as f32 * (L as f32 - 1.0) / (H as f32 - 1.0); // fractional index position, similar to fractional delay line.
        let idx = pos.floor() as usize;
        let frac = pos - idx as f32; // get the difference between the f32 index, and the actual floor usize position

        let y1 = inputs[idx];
        let y2 = inputs.get(idx + 1).unwrap_or(&y1); // just grab the last value in buffer incase that would be out of bounds

        outputs[i] = y1 + (y2 - y1) * frac;
    }
}

#[inline(always)]
pub fn downsample_first_sample<const L: usize, const H: usize>(inputs: &Buffer<H>, outputs: &mut Buffer<L>){
    debug_assert!(L < H);
    debug_assert!(H % L == 0);

    let r: usize = H / L;

    for i in 0..L {
        outputs[i] = inputs[i * r];
    }
}

#[cfg(test)]
mod test {
    use crate::engine::{buffer::Buffer, utils::{downsample_first_sample, upsample_linear, upsample_zoh}};

    const MAX_ERROR: f32 = 0.0001;

    fn assert_roughly_eq(a: &[f32], b: &[f32]){
        a.iter().zip(b).for_each(|(i, j)| {
            if (i - j).abs() > MAX_ERROR {
                panic!("Greater than maximum difference found!: {} : {}", i, j);
            }
        });
    }

    #[test]
    fn sanity(){
        let inputs = [0.0, 1.0, 2.0];
        assert_roughly_eq(&inputs, &inputs.clone());
    }

    #[test]
    fn assert_upsample_zoh(){
        let inputs: Buffer<3> = Buffer { data: [0.0, 0.5, 1.0] } ;                        // N = 3
        let mut outputs: Buffer<6> = Buffer { data: [0.0; 6] };                           // N = 6
        let expected_outputs = Buffer { data: [0.0, 0.0, 0.5, 0.5, 1.0, 1.0] };           // N = 6

        upsample_zoh::<3, 6>(&inputs, &mut outputs);

        println!("{:?}", outputs);
        println!("{:?}", expected_outputs);

        assert_roughly_eq(&expected_outputs, &outputs);
    }

    #[test]
    fn assert_upsample_linear(){
        let inputs: Buffer<3> = Buffer { data: [0.0, 0.5, 1.0] } ;                         // N = 3
        let mut outputs: Buffer<6> = Buffer { data: [0.0; 6] };                            // N = 6
        let expected_outputs = Buffer { data: [0.0, 0.2, 0.4, 0.6, 0.8, 1.0] };            // N = 6

        upsample_linear::<3, 6>(&inputs, &mut outputs);

        println!("{:?}", outputs);
        println!("{:?}", expected_outputs);

        assert_roughly_eq(&expected_outputs, &outputs);
    }

    #[test]
    fn assert_downsample_first(){
        let inputs: Buffer<10> = Buffer { data: [1.0,2.0,3.0,4.0,5.0,6.0,7.0,8.0,9.0,10.0] }; // N = 10
        let mut outputs: Buffer<5> = Buffer { data: [0.0; 5] };                               // N = 5

        let expected_outputs: Buffer<5> = Buffer { data: [1.0, 3.0, 5.0, 7.0, 9.0] };         // N = 5


        downsample_first_sample::<5, 10>(&inputs, &mut outputs);

        assert_roughly_eq(&expected_outputs, &outputs);
    }
}