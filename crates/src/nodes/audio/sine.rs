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

use std::simd::{Simd, StdFloat};

use crate::{
    context::AudioContext,
    msg::{NodeMessage, RtValue},
    node::{Inputs, Node},
    persample::PerSampleNode,
    ports::{PortBuilder, Ports},
    simd::{LANES, Vf32},
};

/// Polynomial order used for the sine approximation. Higher is more spectrally
/// pure; lower is cheaper. Low/Med are intended for audio-rate modulation
/// sources (e.g. delay-line modulators) where harmonic purity is inaudible.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Quality {
    /// order-3, ~5% peak error
    Low,
    /// order-5
    Med,
    /// order-7
    #[default]
    High,
}

impl Quality {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "low" | "3" => Some(Quality::Low),
            "med" | "medium" | "5" => Some(Quality::Med),
            "high" | "7" => Some(Quality::High),
            _ => None,
        }
    }
}

#[derive(Clone)]
pub struct Sine {
    freq: f32,
    phase: f32,
    quality: Quality,
    sr: f32,
    ports: Ports,
}

impl Sine {
    pub fn new(freq: f32, sr: f32) -> Self {
        Self::with_quality(freq, sr, Quality::default())
    }

    pub fn with_quality(freq: f32, sr: f32, quality: Quality) -> Self {
        Self {
            freq,
            phase: 0.0,
            quality,
            sr,
            ports: PortBuilder::default()
                .audio_in_named(&["freq"])
                .audio_out(1)
                .build(),
        }
    }

    #[inline(always)]
    fn tick_inner<const ORDER: usize>(&mut self, freq: f32) -> f32 {
        // Multiply by the reciprocal, like the block path, so the phase
        // increment is bit-identical and does not drift against `process`.
        self.phase = self.phase.fract() + freq * (1.0 / self.sr);
        sin_turns::<ORDER, 1>(Simd::splat(self.phase)).as_array()[0]
    }

    fn process_external_freq<const ORDER: usize>(
        &mut self,
        ctx: &mut AudioContext,
        fm_in: &[f32],
        ao: &mut [&mut [f32]],
    ) {
        let config = ctx.get_config();

        let fs_recipricol = Vf32::splat(1.0 / config.sample_rate as f32);

        for (n, freq_chunk) in fm_in.chunks_exact(LANES).enumerate() {
            let freq = Vf32::from_slice(freq_chunk);

            let mut inc = freq * fs_recipricol;
            inc = simd_scan(inc);

            let mut phase = Simd::splat(self.phase.fract());
            phase += inc;

            self.phase = phase.as_array()[LANES - 1];

            let sample = sin_turns::<ORDER, LANES>(phase);

            let start = n * LANES;
            let end = start + LANES;

            let sample_arr = sample.as_array();

            for chan in ao.iter_mut() {
                chan[start..end].copy_from_slice(sample_arr);
            }
        }
    }

    fn process_internal_freq<const ORDER: usize>(
        &mut self,
        ctx: &mut AudioContext,
        ao: &mut [&mut [f32]],
    ) {
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

            let sample = sin_turns::<ORDER, LANES>(phase);

            let start = i * LANES;
            let end = start + LANES;

            for chan in ao.iter_mut() {
                chan[start..end].copy_from_slice(sample.as_array());
            }
        }
    }
}

impl PerSampleNode for Sine {
    fn ports(&self) -> &Ports {
        &self.ports
    }

    fn tick(&mut self, in_frame: &[Option<f32>], out_frame: &mut [f32]) {
        let freq = in_frame[0].unwrap_or(self.freq);
        let sample = match self.quality {
            Quality::High => self.tick_inner::<7>(freq),
            Quality::Med => self.tick_inner::<5>(freq),
            Quality::Low => self.tick_inner::<3>(freq),
        };
        for out in out_frame.iter_mut() {
            *out = sample;
        }
    }

    fn handle_msg(&mut self, msg: NodeMessage) {
        Node::handle_msg(self, msg);
    }
}

impl Node for Sine {
    fn process(&mut self, ctx: &mut AudioContext, ai: &Inputs, ao: &mut [&mut [f32]]) {
        match (self.quality, ai[0]) {
            (Quality::High, Some(fm)) => self.process_external_freq::<7>(ctx, fm, ao),
            (Quality::High, None) => self.process_internal_freq::<7>(ctx, ao),
            (Quality::Med, Some(fm)) => self.process_external_freq::<5>(ctx, fm, ao),
            (Quality::Med, None) => self.process_internal_freq::<5>(ctx, ao),
            (Quality::Low, Some(fm)) => self.process_external_freq::<3>(ctx, fm, ao),
            (Quality::Low, None) => self.process_internal_freq::<3>(ctx, ao),
        }
    }

    /// For now, we panic here, as it's difficult to make a strong message without allocating
    fn handle_msg(&mut self, msg: crate::msg::NodeMessage) {
        if let NodeMessage::SetParam(payload) = msg {
            match (payload.param_name, payload.value) {
                ("freq", RtValue::F32(val)) => self.freq = val,
                _ => unreachable!("Invalid parameter and value passed"),
            }
        }
    }

    fn ports(&self) -> &Ports {
        &self.ports
    }
}

use crate::{
    builder::{ResourceBuilderView, ValidationError},
    dsl::ir::DSLParams,
    node::DynNode,
    spec::NodeDefinition,
};

impl Sine {
    pub fn from_params(
        rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Self, ValidationError> {
        let freq = p.get_f32("freq").unwrap_or(440.0);
        let quality = p
            .get_str("quality")
            .map(|s| {
                Quality::from_str(&s).ok_or_else(|| {
                    ValidationError::InvalidParameter(
                        "quality must be one of: low/3, med/5, high/7".into(),
                    )
                })
            })
            .transpose()?
            .unwrap_or_default();
        let sr = rb.get_config().sample_rate as f32;
        Ok(Self::with_quality(freq, sr, quality))
    }
}

impl NodeDefinition for Sine {
    const NAME: &'static str = "sine";
    const DESCRIPTION: &'static str = "Sine wave oscillator with optional FM input";
    const REQUIRED_PARAMS: &'static [&'static str] = &[];
    const OPTIONAL_PARAMS: &'static [&'static str] = &["freq", "chans", "quality"];

    fn create(
        rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        Ok(Box::new(Self::from_params(rb, p)?))
    }
}

// Start of BSD-3 Code

#[inline(always)]
fn fast_mod_mhalf_half<const LANES: usize>(x: Simd<f32, LANES>) -> Simd<f32, LANES> {
    x - x.round()
}

#[inline(always)]
fn sin_turns_mhalfpi_halfpi<const ORDER: usize, const LANES: usize>(
    x: Simd<f32, LANES>,
) -> Simd<f32, LANES> {
    let x_sq = x * x;

    let y = match ORDER {
        3 => {
            // -24.6941916306 x + 50.1403295328 x^3
            let x_1_3 = Simd::splat(-24.694_19_f32) + Simd::splat(50.140_33_f32) * x_sq;
            x * x_1_3
        }
        5 => {
            // -25.1167285815 x + 63.6615119634 x^3 - 54.0847297225 x^5
            let x_3_5 = Simd::splat(63.661_51_f32) + Simd::splat(-54.084_73_f32) * x_sq;
            let x_1_3_5 = Simd::splat(-25.116_73_f32) + x_3_5 * x_sq;
            x * x_1_3_5
        }
        _ => {
            // order-7: -25.1323666662 x + 64.7874540567 x^3 - 66.0947787168 x^5 + 32.0267973181 x^7
            let x_q = x_sq * x_sq;
            let x_5_7 = Simd::splat(-66.094_78_f32) + Simd::splat(32.026_8_f32) * x_sq;
            let x_1_3 = Simd::splat(-25.132_366_f32) + Simd::splat(64.787_45_f32) * x_sq;
            let x_1_3_5_7 = x_1_3 + x_5_7 * x_q;
            x * x_1_3_5_7
        }
    };

    y * (x + Simd::splat(0.5)) * (x - Simd::splat(0.5))
}

#[inline(always)]
fn sin_turns<const ORDER: usize, const LANES: usize>(x: Simd<f32, LANES>) -> Simd<f32, LANES> {
    let x_wrapped = fast_mod_mhalf_half(x);
    sin_turns_mhalfpi_halfpi::<ORDER, LANES>(x_wrapped)
}

pub fn simd_scan<const LANES: usize>(mut x: Simd<f32, LANES>) -> Simd<f32, LANES> {
    // TODO: a nicer way
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
    use super::{simd_scan, sin_turns};

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

    fn max_err<const ORDER: usize>() -> f32 {
        let mut worst = 0.0f32;
        for i in 0..1000 {
            let turns = i as f32 / 1000.0 - 0.5;
            let approx = sin_turns::<ORDER, 1>(std::simd::Simd::splat(turns)).as_array()[0];
            let exact = (turns * std::f32::consts::TAU).sin();
            worst = worst.max((approx - exact).abs());
        }
        worst
    }

    #[test]
    fn quality_orders_bounded() {
        assert!(max_err::<7>() < 1e-3, "order-7 too inaccurate");
        assert!(max_err::<5>() < 1e-2, "order-5 too inaccurate");
        assert!(max_err::<3>() < 1e-1, "order-3 too inaccurate");
    }
}
