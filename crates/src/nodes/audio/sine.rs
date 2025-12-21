// While this project is AGPLv3, this file includes code under BSD-3 

// Approximations adapted from Chowdhurry-DSP, license below

// BSD 3-Clause License

// Copyright (c) 2024, jatinchowdhury18
// All rights reserved.

// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions are met:

// 1. Redistributions of source code must retain the above copyright notice, this
//    list of conditions and the following disclaimer.

// 2. Redistributions in binary form must reproduce the above copyright notice,
//    this list of conditions and the following disclaimer in the documentation
//    and/or other materials provided with the distribution.

// 3. Neither the name of the copyright holder nor the names of its
//    contributors may be used to endorse or promote products derived from
//    this software without specific prior written permission.

// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
// AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
// IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
// DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
// FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
// DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
// SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
// CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
// OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
// OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::simd::{LaneCount, Simd, StdFloat, SupportedLaneCount};

use crate::{
    context::AudioContext, msg::{NodeMessage, RtValue}, node::{Channels, Inputs, Node}, ports::{PortBuilder, Ports}, simd::{LANES, Vf32}
};

#[derive(Clone)]
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
            .audio_in_named(&["freq"])
            .audio_out(chans)
            .build()
        }
    }
    fn process_external_freq(&mut self, ctx: &mut AudioContext, fm_in: &[f32], ao: &mut Channels){
        let config = ctx.get_config();

        let fs_recipricol = Vf32::splat(1.0 / config.sample_rate as f32);

        for (n, freq_chunk) in fm_in.chunks_exact(LANES).enumerate() {
            let freq = Vf32::from_slice(freq_chunk);

            let mut inc = freq * fs_recipricol;
            inc = simd_scan(inc);

            let mut phase = Simd::splat(self.phase.fract());
            phase += inc;

            self.phase = phase.as_array()[LANES - 1];

            let sample = sin_turns_7(phase);

            let start = n * LANES;
            let end = start + LANES;

            let sample_arr = sample.as_array();

            for chan in ao.iter_mut() {
                chan[start..end].copy_from_slice(sample_arr);
            }
        }
    }

    fn process_internal_freq(&mut self, ctx: &mut AudioContext, ao: &mut Channels){
        let config = ctx.get_config();
        let freq = Vf32::splat(self.freq);

        let fs_recipricol = Vf32::splat(1.0 / config.sample_rate as f32);

        let block_size = config.block_size;
        let n = block_size / LANES;

        for i in 0..n {
            let mut inc = freq * fs_recipricol;
            
            inc = simd_scan(inc);

            let mut phase = Simd::splat(self.phase.fract());
            phase += inc;

            self.phase = phase.as_array()[LANES - 1];

            let sample = sin_turns_7(phase);

            let start = i * LANES;
            let end = start + LANES;

            for chan in ao.iter_mut() {
                chan[start..end].copy_from_slice(sample.as_array());
            }
        }
    }
}

impl Node for Sine {
    fn process(
        &mut self,
        ctx: &mut AudioContext,
        ai: &Inputs,
        ao: &mut Channels,
    ) {
        if let Some(fm_in) = ai[0] {
            self.process_external_freq(ctx, fm_in, ao);
        }
        else {
            self.process_internal_freq(ctx, ao);
        }
    }

    /// For now, we panic here, as it's difficult to make a strong message without allocating
    fn handle_msg(&mut self, msg: crate::msg::NodeMessage) {
        match msg {
            NodeMessage::SetParam(payload) => {
                match (payload.param_name, payload.value) {
                    ("freq", RtValue::F32(val)) => self.freq = val,
                    _ => unreachable!("Invalid parameter and value passed")
                }
            }
        }
    }
    
    fn ports(&self) -> &Ports {
        &self.ports
    }
}

// Start of BSD-3 Code

#[inline(always)]
fn fast_mod_mhalf_half<const LANES: usize>(x: Simd<f32, LANES>) -> Simd<f32, LANES>
where
    LaneCount<LANES>: SupportedLaneCount,
{
    x - x.round()
}

#[inline(always)]
fn sin_turns_mhalfpi_halfpi_7<const LANES: usize>(x: Simd<f32, LANES>) -> Simd<f32, LANES>
where
    LaneCount<LANES>: SupportedLaneCount,
{
    let x_sq = x * x;
    let x_q = x_sq * x_sq;

    let c1 = Simd::splat(-25.132_366_f32);
    let c3 = Simd::splat(64.787_45_f32);
    let c5 = Simd::splat(-66.094_78_f32);
    let c7 = Simd::splat(32.026_8_f32);

    let x_5_7 = c5 + c7 * x_sq;
    let x_1_3 = c1 + c3 * x_sq;
    let x_1_3_5_7 = x_1_3 + x_5_7 * x_q;

    let y = x * x_1_3_5_7;
    y * (x + Simd::splat(0.5)) * (x - Simd::splat(0.5))
}

#[inline(always)]
fn sin_turns_7<const LANES: usize>(x: Simd<f32, LANES>) -> Simd<f32, LANES>
where
    LaneCount<LANES>: SupportedLaneCount,
{
    let x_wrapped = fast_mod_mhalf_half(x);
    sin_turns_mhalfpi_halfpi_7(x_wrapped)
}

// End of BSD-3 Code

/// Utility to perform prefix scan
fn simd_scan<const LANES: usize>(mut x: Simd<f32, LANES>) -> Simd<f32, LANES>
where
    LaneCount<LANES>: SupportedLaneCount,
{
    let t1 = x.shift_elements_right::<1>(0.0);
    x += t1;

    let t2 = x.shift_elements_right::<2>(0.0);
    x += t2;

    if LANES >= 4 {
        let t4 = x.shift_elements_right::<4>(0.0);
        x += t4;
    }

    if LANES >= 8 {
        let t8 = x.shift_elements_right::<8>(0.0);
        x += t8;
    }

    x
}

#[cfg(test)]
mod test {
    use crate::nodes::audio::sine::simd_scan;

    #[test]
    fn check_prefix_sum_block_simd() {
        let input = std::simd::Simd::<f32, 1>::from_array([1.0]);
        let expected = std::simd::Simd::<f32, 1>::from_array([1.0]);

        let input_one = std::simd::Simd::<f32, 2>::from_array([1.0, 2.0]);
        let expected_one = std::simd::Simd::<f32, 2>::from_array([1.0, 3.0]);

        let input_two = std::simd::Simd::<f32, 4>::from_array([1.0, 3.0, 5.0, 9.0]);
        let expected_two = std::simd::Simd::<f32, 4>::from_array([1.0, 4.0, 9.0, 18.0]);

        let input_three = std::simd::Simd::<f32, 8>::from_array([1.0; 8]);
        let expected_three =
            std::simd::Simd::<f32, 8>::from_array(std::array::from_fn(|i| (i + 1) as f32));

        let input_four = std::simd::Simd::<f32, 16>::from_array([1.0; 16]);
        let expected_four =
            std::simd::Simd::<f32, 16>::from_array(std::array::from_fn(|i| (i + 1) as f32));

        assert_eq!(expected, simd_scan(input));
        assert_eq!(expected_one, simd_scan(input_one));
        assert_eq!(expected_two, simd_scan(input_two));
        assert_eq!(expected_three, simd_scan(input_three));
        assert_eq!(expected_four, simd_scan(input_four));
    }
}
