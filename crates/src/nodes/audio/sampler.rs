use crate::{
    builder::{ResourceBuilderView, ValidationError},
    dsl::ir::DSLParams,
    math::cubic_hermite,
    node::{DynNode, Inputs, Node},
    nodes::midi::voice::MIDDLE_C,
    ports::{PortBuilder, Ports},
    resources::{ExternalBufferKey, buffer::ExternalBuffer},
    spec::NodeDefinition,
};
const TRIG: usize = 0;
const SPEED: usize = 1;
const PITCH: usize = 2;
const START: usize = 3;
const END: usize = 4;

/// A more creative sampler implementation with fractional playheads,
/// variable playhead speed, audio-rate bounds, etc.
///
/// `speed` is a playback ratio where 1.0 is original pitch, 2.0 is an octave up,
///  and negative values play in reverse.
///
/// `pitch` is a frequency in Hz, with middle C. If freq and speed are both present
/// we default to just using the frequency input.
///
/// `start` and `end` are in samples, with `end` exclusive. When the
/// playhead leaves the slice the we output silence until the next trig.
///
/// Using cubic-hermite interpolation here for the time being. Perhaps some
/// developers may want an adjustable rate here for platforms with less compute?
///
/// Repitching here will change the sound of the sample as we are not using
/// any clever math or implementations for repitching other than changing
/// the playhead speed.
///
/// Higher quality, more expensive approaches will be under consideration
/// in the future.
///
/// For the time being, if you seek more natural repitching, the [`Grain`]
/// node with a more hann-esque shape acts as a reasonable instrument that
/// can repitch. This is however a naive OLA algorithm and not suitable say
/// for DJing, mixing and mastering.
#[derive(Clone)]
pub struct Sampler {
    start: usize,
    end: usize,
    speed: f32,
    playhead: f32,
    active: bool,
    last_trig: f32,
    /// Used to load shared resources, in this case an [`ExternalBuffer`].
    /// This is an Arc<[f32]> loaded in by another thread; swapped-out buffers
    /// are returned to the frontend to drop.
    sample_key: ExternalBufferKey,
    ports: Ports,
}

impl Sampler {
    pub fn new(
        sample_key: ExternalBufferKey,
        start: usize,
        end: usize,
        speed: f32,
        chans: usize,
    ) -> Self {
        Self {
            start,
            end,
            speed,
            sample_key,
            playhead: start as f32,
            active: true,
            last_trig: 0.0,
            ports: PortBuilder::default()
                .control_in_named(&["trig", "speed", "pitch", "start", "end"])
                .audio_out(chans)
                .build(),
        }
    }

    pub fn with_pitch(mut self, freq: f32) -> Self {
        self.speed = freq_to_ratio(freq);
        self
    }

    /// Advances the playhead one frame, returning the position to read, or
    /// `None` when the playhead is outside the region.
    #[inline(always)]
    fn next_position(&mut self, inputs: &Inputs, i: usize, len: f32) -> Option<f32> {
        let at = |port: usize, default: f32| inputs[port].map_or(default, |x| x[i]);

        // min/max rather than clamp so we don't panic
        let end = at(END, self.end as f32).max(0.0).min(len);
        let start = at(START, self.start as f32).max(0.0).min(end);

        let speed = match inputs[PITCH] {
            Some(pitch) => freq_to_ratio(pitch[i]),
            None => at(SPEED, self.speed),
        };

        let trig = at(TRIG, 0.0);

        if trig >= 0.5 && self.last_trig < 0.5 {
            self.playhead = if speed < 0.0 { end - 1.0 } else { start };
            self.active = true;
        }
        self.last_trig = trig;

        self.active &= (start..end).contains(&self.playhead);
        if !self.active {
            return None;
        }

        let pos = self.playhead;
        self.playhead += speed;
        Some(pos)
    }

    /// Renders one block from `buffer`, which must be non-empty.
    pub fn render(&mut self, buffer: &ExternalBuffer, inputs: &Inputs, outputs: &mut [&mut [f32]]) {
        let len = buffer.len_samples();
        let block_size = outputs.first().map_or(0, |c| c.len());

        for i in 0..block_size {
            let pos = self.next_position(inputs, i, len as f32);
            for (c, chan_out) in outputs.iter_mut().enumerate() {
                let src = buffer.channel(c % buffer.num_channels);
                chan_out[i] = pos.map_or(0.0, |p| read_cubic(src, p));
            }
        }
    }
}

#[inline(always)]
fn freq_to_ratio(freq: f32) -> f32 {
    freq / MIDDLE_C
}

#[inline(always)]
fn read_cubic(src: &[f32], pos: f32) -> f32 {
    let last = src.len() as isize - 1;
    let index = pos.floor() as isize;
    let tap = |offset: isize| src[(index + offset).clamp(0, last) as usize];
    cubic_hermite(tap(-1), tap(0), tap(1), tap(2), pos - pos.floor())
}

impl Node for Sampler {
    fn process(
        &mut self,
        ctx: &mut crate::prelude::AudioContext,
        inputs: &Inputs,
        outputs: &mut [&mut [f32]],
    ) {
        match ctx.get_resources().get_external_buffer(self.sample_key) {
            Some(buffer) if buffer.num_channels > 0 && buffer.len_samples() > 0 => {
                self.render(buffer, inputs, outputs)
            }
            _ => outputs.iter_mut().for_each(|c| c.fill(0.0)),
        }
    }
    fn handle_msg(&mut self, _msg: crate::msg::NodeMessage) {}
    fn ports(&self) -> &Ports {
        &self.ports
    }
}

impl NodeDefinition for Sampler {
    const NAME: &'static str = "sampler";
    const DESCRIPTION: &'static str =
        "Plays back a loaded audio sample with variable speed, pitch and region";
    const REQUIRED_PARAMS: &'static [&'static str] = &["sampler_name"];
    const OPTIONAL_PARAMS: &'static [&'static str] = &["chans", "speed", "pitch", "start", "end"];

    fn create(
        rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        let name = p.require_str("sampler_name")?;
        let chans = p.get_usize("chans")?.unwrap_or(2);
        let speed = p.get_f32("speed")?.unwrap_or(1.0);
        let start = p.get_usize("start")?.unwrap_or(0);
        let end = p.get_usize("end")?.unwrap_or(usize::MAX);
        let key = rb.add_external_buffer_key(&name);

        let node = Self::new(key, start, end, speed, chans);
        let node = match p.get_f32("pitch")? {
            Some(freq) => node.with_pitch(freq),
            None => node,
        };
        Ok(Box::new(node))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOCK: usize = 64;

    fn ramp_buffer(len: usize, chans: usize) -> ExternalBuffer {
        ExternalBuffer {
            data: (0..chans)
                .flat_map(|_| (0..len).map(|x| x as f32))
                .collect(),
            num_channels: chans,
        }
    }

    fn sampler(start: usize, end: usize, speed: f32) -> Sampler {
        Sampler::new(ExternalBufferKey::default(), start, end, speed, 2)
    }

    fn render(node: &mut Sampler, buffer: &ExternalBuffer, inputs: &Inputs) -> [Vec<f32>; 2] {
        let mut l = vec![f32::NAN; BLOCK];
        let mut r = vec![f32::NAN; BLOCK];
        node.render(buffer, inputs, &mut [&mut l, &mut r]);
        [l, r]
    }

    #[test]
    fn unit_speed_reproduces_buffer_on_every_channel() {
        let buffer = ramp_buffer(BLOCK * 2, 2);
        let [l, r] = render(&mut sampler(0, BLOCK * 2, 1.0), &buffer, &[None; 5]);
        let expected: Vec<f32> = (0..BLOCK).map(|x| x as f32).collect();
        assert_eq!(l, expected);
        assert_eq!(r, expected);
    }

    #[test]
    fn half_speed_interpolates_between_samples() {
        let buffer = ramp_buffer(BLOCK * 2, 2);
        let [l, _] = render(&mut sampler(4, BLOCK * 2, 0.5), &buffer, &[None; 5]);
        for (i, y) in l.iter().enumerate() {
            assert!((y - (4.0 + i as f32 * 0.5)).abs() < 1e-4, "frame {i}: {y}");
        }
    }

    #[test]
    fn silent_after_region_end_and_at_buffer_edges_without_panicking() {
        let buffer = ramp_buffer(5, 1);
        let [l, r] = render(&mut sampler(0, 5, 1.3), &buffer, &[None; 5]);
        assert!(l.iter().chain(&r).all(|y| y.is_finite()));
        assert!(l[4..].iter().all(|&y| y == 0.0));
        assert_eq!(l, r);
    }

    #[test]
    fn region_past_buffer_end_is_clamped() {
        let buffer = ramp_buffer(8, 2);
        let [l, _] = render(&mut sampler(0, 10_000, 1.0), &buffer, &[None; 5]);
        assert!(l[8..].iter().all(|&y| y == 0.0));
    }

    #[test]
    fn rising_edge_restarts_and_held_trig_does_not() {
        let buffer = ramp_buffer(BLOCK * 2, 2);
        let mut trig = vec![0.0; BLOCK];
        trig[10..20].fill(1.0);
        let [l, _] = render(
            &mut sampler(0, BLOCK * 2, 1.0),
            &buffer,
            &[Some(&trig), None, None, None, None],
        );
        assert_eq!(l[9], 9.0);
        assert_eq!(l[10], 0.0);
        assert_eq!(l[19], 9.0);
    }

    #[test]
    fn reverse_trig_starts_from_region_end() {
        let buffer = ramp_buffer(BLOCK * 2, 2);
        let mut trig = vec![0.0; BLOCK];
        trig[0] = 1.0;
        let [l, _] = render(
            &mut sampler(0, 16, -1.0),
            &buffer,
            &[Some(&trig), None, None, None, None],
        );
        let expected: Vec<f32> = (0..16).rev().map(|x| x as f32).collect();
        assert_eq!(l[..16], expected);
        assert!(l[16..].iter().all(|&y| y == 0.0));
    }

    #[test]
    fn nan_bounds_do_not_panic() {
        let buffer = ramp_buffer(BLOCK, 2);
        let nan = vec![f32::NAN; BLOCK];
        let [l, _] = render(
            &mut sampler(0, BLOCK, 1.0),
            &buffer,
            &[None, None, None, Some(&nan), Some(&nan)],
        );
        assert!(l.iter().all(|y| y.is_finite()));
    }

    #[test]
    fn pitch_input_takes_priority_over_speed() {
        let buffer = ramp_buffer(BLOCK * 4, 2);
        let speed = vec![3.0; BLOCK];
        let octave_up = vec![MIDDLE_C * 2.0; BLOCK];
        let [l, _] = render(
            &mut sampler(0, BLOCK * 4, 1.0),
            &buffer,
            &[None, Some(&speed), Some(&octave_up), None, None],
        );
        let expected: Vec<f32> = (0..BLOCK).map(|x| x as f32 * 2.0).collect();
        assert_eq!(l, expected);
    }

    #[test]
    fn middle_c_plays_at_unit_speed() {
        let buffer = ramp_buffer(BLOCK * 2, 2);
        let middle_c = vec![MIDDLE_C; BLOCK];
        let [l, _] = render(
            &mut sampler(0, BLOCK * 2, 3.0),
            &buffer,
            &[None, None, Some(&middle_c), None, None],
        );
        let expected: Vec<f32> = (0..BLOCK).map(|x| x as f32).collect();
        assert_eq!(l, expected);
    }

    #[test]
    fn with_pitch_sets_default_speed() {
        let buffer = ramp_buffer(BLOCK * 2, 2);
        let mut node = sampler(0, BLOCK * 2, 1.0).with_pitch(MIDDLE_C / 2.0);
        let [l, _] = render(&mut node, &buffer, &[None; 5]);
        assert_eq!(l[2], 1.0);
    }
}
