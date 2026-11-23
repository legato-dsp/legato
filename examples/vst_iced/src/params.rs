use legato::nodes::audio::grain_delay::{MAX_OVERLAP, MAX_PITCH_SEMITONES};
use nice_plug::prelude::*;

/// Matches `max_delay_length` in `.legato`: a whole note at 60 bpm.
pub const MAX_DELAY_MS: f32 = 4000.0;

/// The ids keep saved projects compatible if variants are added or reordered.
#[derive(Enum, Clone, Copy, Debug, PartialEq)]
pub enum Division {
    #[id = "1/32"]
    #[name = "1/32"]
    ThirtySecond,
    #[id = "1/16t"]
    #[name = "1/16T"]
    SixteenthTriplet,
    #[id = "1/16"]
    #[name = "1/16"]
    Sixteenth,
    #[id = "1/16d"]
    #[name = "1/16D"]
    DottedSixteenth,
    #[id = "1/8t"]
    #[name = "1/8T"]
    EighthTriplet,
    #[id = "1/8"]
    #[name = "1/8"]
    Eighth,
    #[id = "1/8d"]
    #[name = "1/8D"]
    DottedEighth,
    #[id = "1/4t"]
    #[name = "1/4T"]
    QuarterTriplet,
    #[id = "1/4"]
    #[name = "1/4"]
    Quarter,
    #[id = "1/4d"]
    #[name = "1/4D"]
    DottedQuarter,
    #[id = "1/2"]
    #[name = "1/2"]
    Half,
    #[id = "1/1"]
    #[name = "1/1"]
    Whole,
}

impl Division {
    /// Length in quarter notes, the unit hosts report positions in.
    pub fn beats(self) -> f64 {
        match self {
            Self::ThirtySecond => 1.0 / 8.0,
            Self::SixteenthTriplet => 1.0 / 6.0,
            Self::Sixteenth => 1.0 / 4.0,
            Self::DottedSixteenth => 3.0 / 8.0,
            Self::EighthTriplet => 1.0 / 3.0,
            Self::Eighth => 1.0 / 2.0,
            Self::DottedEighth => 3.0 / 4.0,
            Self::QuarterTriplet => 2.0 / 3.0,
            Self::Quarter => 1.0,
            Self::DottedQuarter => 3.0 / 2.0,
            Self::Half => 2.0,
            Self::Whole => 4.0,
        }
    }

    pub fn ms(self, tempo: f64) -> f32 {
        (self.beats() * 60_000.0 / tempo) as f32
    }
}

#[derive(Params)]
pub struct GrainDelayParams {
    #[id = "delay_sync"]
    pub delay_sync: BoolParam,
    #[id = "delay_division"]
    pub delay_division: EnumParam<Division>,
    #[id = "delay_time"]
    pub delay_time: FloatParam,
    #[id = "sync"]
    pub sync: BoolParam,
    #[id = "division"]
    pub division: EnumParam<Division>,
    #[id = "grain_size"]
    pub grain_size: FloatParam,
    #[id = "grains"]
    pub grains: IntParam,
    #[id = "pitch"]
    pub pitch: FloatParam,
    #[id = "shape"]
    pub shape: FloatParam,
    #[id = "jitter"]
    pub jitter: FloatParam,
    #[id = "feedback"]
    pub feedback: FloatParam,
    #[id = "damping"]
    pub damping: FloatParam,
    #[id = "diffusion"]
    pub diffusion: FloatParam,
    #[id = "reverb_decay"]
    pub reverb_decay: FloatParam,
    #[id = "reverb_mix"]
    pub reverb_mix: FloatParam,
    #[id = "mix"]
    pub mix: FloatParam,
}

impl Default for GrainDelayParams {
    /// Defaults match the `grains` and `verb` nodes in `.legato`.
    fn default() -> Self {
        let unit = |name, default, min, max| {
            FloatParam::new(name, default, FloatRange::Linear { min, max })
                .with_value_to_string(formatters::v2s_f32_rounded(3))
        };
        let percent = |name, default| {
            FloatParam::new(name, default, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit("%")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage())
        };
        let millis = |name, default, max| {
            FloatParam::new(
                name,
                default,
                FloatRange::Skewed {
                    min: 10.0,
                    max,
                    factor: FloatRange::skew_factor(-1.0),
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(0))
        };
        Self {
            delay_sync: BoolParam::new("Delay Sync", false),
            delay_division: EnumParam::new("Delay Division", Division::DottedEighth),
            delay_time: millis("Delay Time", 1000.0, MAX_DELAY_MS),
            sync: BoolParam::new("Grain Sync", false),
            division: EnumParam::new("Grain Division", Division::Eighth),
            // Long grains pitched down drift far through the delay line, so we cap
            // this well below the node's 3s limit to keep a 1s delay usable.
            grain_size: millis("Grain Size", 400.0, 1000.0),
            grains: IntParam::new(
                "Grains",
                2,
                IntRange::Linear {
                    min: 1,
                    max: MAX_OVERLAP as i32,
                },
            ),
            pitch: FloatParam::new(
                "Pitch",
                0.0,
                FloatRange::Linear {
                    min: -MAX_PITCH_SEMITONES,
                    max: MAX_PITCH_SEMITONES,
                },
            )
            .with_step_size(1.0)
            .with_unit(" st")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),
            shape: unit("Shape", 0.01, 0.001, 0.5),
            jitter: percent("Jitter", 0.0),
            feedback: percent("Feedback", 0.8),
            damping: unit("Damping", 0.8, 0.0001, 0.999),
            diffusion: percent("Diffusion", 1.0),
            reverb_decay: unit("Reverb Decay", 0.8, 0.0, 0.9),
            reverb_mix: percent("Reverb Mix", 0.8),
            mix: percent("Mix", 0.5),
        }
    }
}
