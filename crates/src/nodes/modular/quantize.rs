use crate::{
    builder::{ResourceBuilderView, ValidationError},
    context::AudioContext,
    dsl::ir::{DSLParams, Value},
    node::{DynNode, Inputs, Node},
    nodes::midi::voice::mtof_f32,
    persample::PerSampleNode,
    ports::{PortBuilder, Ports},
    spec::NodeDefinition,
};

/// Snaps a continuous pitch onto the nearest note of a 12-bit scale mask.
#[derive(Clone, Copy, Debug)]
pub struct Quantizer {
    /// The 12-bit mask replicated across three octavesxw.
    mask: u64,
}

impl Quantizer {
    /// Create a new note quantizer with a u16 mask.
    ///
    /// The mask here is a 12 bit array, where each bit is a note shifted from c
    ///                            C          E         G
    ///                            ↓          ↓         ↓
    /// For example, a mask of (1 << 0) | (1 << 4) | (1 << 7) would quantize to
    /// a c major triad.
    ///
    /// Returns `None` for an empty mask: a scale with no notes in it has no
    /// nearest note, and every step below assumes at least one bit is set.
    pub fn new(mask: u16) -> Option<Self> {
        // Strip the 12 notes we actually need
        let m = (mask & 0x0fff) as u64;
        // This mask replicates the same bit across three different 12 note sections
        // This is useful because we need to handle wrapping up and down
        (m != 0).then(|| Self {
            mask: m * 0x01001001,
        })
    }

    /// Every semitone passes through unchanged.
    pub fn chromatic() -> Self {
        Self::new(0x0fff).unwrap()
    }

    /// The 12-bit pitch-class mask this quantizer snaps to, bit 0 = C.
    pub fn mask(&self) -> u16 {
        (self.mask & 0x0fff) as u16
    }

    /// Whether `semitone`'s pitch class is already in the scale.
    pub fn contains(&self, semitone: i32) -> bool {
        self.mask() & (1 << semitone.rem_euclid(12)) != 0
    }

    /// Snap a continuous MIDI note number to the nearest note of the scale.
    ///
    /// The result is a note number on the same scale as the input.
    ///
    /// Wrapping is handled by searching a mask replicated across three octaves.
    pub fn quantize(&self, semitones: f32) -> f32 {
        // Make sure we have a valid input here
        if !semitones.is_finite() {
            return semitones;
        }

        let oct = (semitones / 12.0).floor();

        let x = semitones - 12.0 * oct;
        let j = (x as u32).min(11) + 12;

        // Find the nearest neighbors of potential note bits.
        debug_assert!((12..=23).contains(&j));

        let lo = 63 - (self.mask & ((1u64 << (j + 1)) - 1)).leading_zeros();
        let hi = (j + 1) + (self.mask >> (j + 1)).trailing_zeros();

        // Get the potential nearest neighbor slots for our semitone
        let (lo, hi) = (lo as f32 - 12.0, hi as f32 - 12.0);

        // Quantize to the nearest, preferring the lower note on a tie.
        12.0 * oct + if x - lo <= hi - x { lo } else { hi }
    }

    /// [`Quantizer::quantize`] with the octave the result lands in.
    pub fn quantize_with_octave(&self, semitones: f32) -> (f32, i32) {
        let note = self.quantize(semitones);
        (note, (note / 12.0).floor() as i32)
    }

    /// Snap to the scale and convert to frequency in Hz.
    pub fn quantize_to_freq(&self, semitones: f32) -> f32 {
        mtof_f32(self.quantize(semitones))
    }
}

/// Cast the name to the semitone, maybe move this into the parser in the future?
///
/// We first grab the whole note, then apply as many flats and sharps as we would like,
/// finally wrapping around 12, as we have a limited set of valid notes, and a B## would
/// wrap upwards to C#.
fn semitone_from_name(name: &str) -> Option<i32> {
    let mut chars = name.chars();
    // Match the note name to the pitch, we later handle accidentals
    let mut semitone: i32 = match chars.next()?.to_ascii_uppercase() {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };

    // Why not let people add as many flats or sharps as possible...
    for accidental in chars {
        semitone += match accidental.to_ascii_lowercase() {
            '#' | 's' => 1,
            'b' => -1,
            _ => return None,
        };
    }

    //
    Some(semitone.rem_euclid(12))
}

fn mask_from_notes(values: &[Value]) -> Result<u16, ValidationError> {
    let mut mask = 0u16;

    for value in values {
        let semitone = match value {
            // TODO: Add a new value type for notes?
            Value::Ident(s) | Value::String(s) => semitone_from_name(s).ok_or_else(|| {
                ValidationError::InvalidParameter(format!(
                    "'{s}' is not a note name in quantize `notes`; expected A-G with optional \
                     accidentals, e.g. C, Eb or \"F#\""
                ))
            })?,
            Value::U32(n) => (*n as i32).rem_euclid(12),
            Value::I32(n) => n.rem_euclid(12),
            other => {
                return Err(ValidationError::InvalidParameter(format!(
                    "quantize `notes` takes note names or semitones, found {other:?}"
                )));
            }
        };

        mask |= 1 << semitone;
    }

    Ok(mask)
}

#[derive(Clone)]
pub struct Quantize {
    quantizer: Quantizer,
    last_freq: f32,
    last_note: f32,
    ports: Ports,
}

impl Quantize {
    pub fn new(quantizer: Quantizer) -> Self {
        Self {
            quantizer,
            last_freq: f32::NEG_INFINITY,
            last_note: f32::NEG_INFINITY,
            ports: PortBuilder::default()
                .control_in(1)
                .default_in()
                .control_out_named(&["freq"])
                .default_out()
                .control_out_named(&["note", "gate"])
                .build(),
        }
    }

    #[inline(always)]
    /// Snap a continuous MIDI note number to the nearest note of the scale.
    ///
    /// Update the last held note, we also launch a trigger event here as well.
    fn update_quantizer(&mut self, pitch: f32) -> Option<(f32, f32)> {
        let note = self.quantizer.quantize(pitch);
        let mut res = None;

        // Update the last note held, we are doing this to launch trig signals.
        // The first sample always fires: `last_note` starts at -inf, so no real
        // note can compare equal to it.
        if note != self.last_note {
            let freq = mtof_f32(note);

            self.last_note = note;
            self.last_freq = freq;

            res = Some((freq, note));
        }

        res
    }

    /// One frame of the node: `(freq, note, trig)` for a single input pitch.
    ///
    /// `trig` is a one-sample impulse — 1.0 on the frame the quantized note
    /// changes, 0.0 for every frame it is held. That is the edge shape the
    /// rest of the graph expects (see `grain`'s `trig` input), so a quantizer
    /// can clock an envelope or a grain each time it lands on a new note.
    ///
    /// Both the block path and the per-sample path go through here so the two
    /// cannot drift; `persample_equivalence` holds them to it.
    #[inline(always)]
    fn tick_inner(&mut self, pitch: f32) -> (f32, f32, f32) {
        match self.update_quantizer(pitch) {
            Some((freq, note)) => (freq, note, 1.0),
            None => (self.last_freq, self.last_note, 0.0),
        }
    }
}

impl Quantize {
    pub fn from_params(
        _rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Self, ValidationError> {
        let values = p.get_array("notes").ok_or_else(|| {
            ValidationError::MissingRequiredParameter(
                "quantize needs `notes`, e.g. notes: [C, E, G] or notes: [0, 4, 7]".to_string(),
            )
        })?;

        let quantizer = Quantizer::new(mask_from_notes(&values)?).ok_or_else(|| {
            ValidationError::InvalidParameter(
                "quantize `notes` must list at least one note".to_string(),
            )
        })?;

        Ok(Self::new(quantizer))
    }
}

impl NodeDefinition for Quantize {
    const NAME: &'static str = "quantize";
    const DESCRIPTION: &'static str = "Snaps a continuous pitch (a MIDI note number) onto the nearest note of a scale, given as `notes: [C, E, G]` or `notes: [0, 4, 7]`. Outputs frequency in Hz on `freq` and the quantized note on `note`.";
    const REQUIRED_PARAMS: &'static [&'static str] = &["notes"];
    const OPTIONAL_PARAMS: &'static [&'static str] = &[];

    fn create(
        rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        Ok(Box::new(Self::from_params(rb, p)?))
    }
}

impl Node for Quantize {
    fn process(&mut self, ctx: &mut AudioContext, inputs: &Inputs, outputs: &mut [&mut [f32]]) {
        let Some(pitch_in) = inputs[0] else { return };

        let block_size = ctx.get_config().block_size;

        for i in 0..block_size {
            let (freq, note, gate) = self.tick_inner(pitch_in[i]);

            outputs[0][i] = freq;
            outputs[1][i] = note;
            outputs[2][i] = gate;
        }
    }

    fn ports(&self) -> &Ports {
        &self.ports
    }
}

impl PerSampleNode for Quantize {
    fn ports(&self) -> &Ports {
        &self.ports
    }

    fn tick(&mut self, in_frame: &[Option<f32>], out_frame: &mut [f32]) {
        let (freq, note, gate) = self.tick_inner(in_frame[0].unwrap_or(0.0));

        out_frame[0] = freq;
        out_frame[1] = note;
        out_frame[2] = gate;
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::nodes::midi::voice::{ftom, mtof};

    const C_MAJOR: u16 = 0b1010_1101_0101;
    const C: u16 = 1 << 0;
    const E: u16 = 1 << 4;
    const GB: u16 = 1 << 6;

    /// A spread of masks to sweep the invariants over.
    fn scales() -> Vec<Quantizer> {
        [
            0x0fff,           // chromatic
            C_MAJOR,          // C major
            C | E | (1 << 7), // C major triad
            0b0010_1001_0101, // A minor pentatonic: A C D E G
            GB,               // a single note, the degenerate case
        ]
        .into_iter()
        .map(|mask| Quantizer::new(mask).unwrap())
        .collect()
    }

    #[test]
    fn neighbours_wrap_through() {
        let q = Quantizer::new(C | E).unwrap();

        assert_eq!(q.quantize(2.4), 4.0);
        assert_eq!(q.quantize(1.9), 0.0);

        assert_eq!(q.quantize(11.7), 12.0); // up into the top octave
        assert_eq!(q.quantize(-0.3), 0.0); // a bit flat and we round up

        assert_eq!(q.quantize(-13.0), -12.0); // round up into the closest octave

        assert_eq!(q.quantize(27.3), 28.0); // Eb~ -> E
        assert_eq!(q.quantize_with_octave(27.3).1, 2);

        assert!(Quantizer::new(0).is_none());
    }

    /// The octave comes from the result, not the input: 11.7 is in octave 0 but
    /// quantizes up into octave 1.
    #[test]
    fn octave_follows_the_quantized_note() {
        let q = Quantizer::new(C | E).unwrap();

        assert_eq!(q.quantize_with_octave(11.7), (12.0, 1));
        assert_eq!(q.quantize_with_octave(-0.3), (0.0, 0));
        assert_eq!(q.quantize_with_octave(-13.0), (-12.0, -1));
        // Only a note strictly below C-1 lands in a negative-numbered octave.
        assert_eq!(q.quantize_with_octave(-25.0), (-24.0, -2));
    }

    #[test]
    fn ties_round_down() {
        let q = Quantizer::new(C | GB).unwrap();
        assert_eq!(q.quantize(3.0), 0.0); // exactly between C and F#
        assert_eq!(q.quantize(9.0), 6.0); // exactly between F# and the next C
    }

    #[test]
    fn chromatic_rounds_to_nearest_semitone() {
        let q = Quantizer::chromatic();

        assert_eq!(q.quantize(60.4), 60.0);
        assert_eq!(q.quantize(60.5), 60.0); // tie rounds down
        assert_eq!(q.quantize(60.6), 61.0);
        assert_eq!(q.quantize(-0.6), -1.0);
    }

    #[test]
    fn non_finite_pitch_passes_through() {
        let q = Quantizer::chromatic();

        assert!(q.quantize(f32::NAN).is_nan());
        assert_eq!(q.quantize(f32::INFINITY), f32::INFINITY);
        assert_eq!(q.quantize(f32::NEG_INFINITY), f32::NEG_INFINITY);
    }

    /// Every quantized note is a whole semitone, is in the scale, and never
    /// moves by more than six semitones — the furthest any pitch can sit from
    /// the nearest note of a scale that repeats every octave.
    #[test]
    fn output_is_always_in_scale_and_nearby() {
        for q in scales() {
            for step in -600..600 {
                let pitch = step as f32 / 10.0;
                let note = q.quantize(pitch);
                let mask = q.mask();

                assert_eq!(note, note.round(), "{mask:03x} @ {pitch}: not a semitone");
                assert!(
                    q.contains(note as i32),
                    "{mask:03x} @ {pitch} -> {note} is out of scale"
                );
                assert!(
                    (note - pitch).abs() <= 6.0,
                    "{mask:03x} @ {pitch} -> {note} moved too far"
                );
            }
        }
    }

    /// Snapping a note that is already in the scale must not move it.
    #[test]
    fn scale_notes_are_fixed_points() {
        for q in scales() {
            for semitone in -24..36 {
                if q.contains(semitone) {
                    let note = semitone as f32;
                    assert_eq!(q.quantize(note), note, "{:03x} moved {note}", q.mask());
                }
            }
        }
    }

    #[test]
    fn quantizing_is_monotonic() {
        for q in scales() {
            let mut prev = f32::NEG_INFINITY;
            for step in -300..300 {
                let note = q.quantize(step as f32 / 7.0);
                assert!(note >= prev, "{:03x} went backwards at {step}", q.mask());
                prev = note;
            }
        }
    }

    // ── Note names ──────────────────────────────────────────────────────────

    #[test]
    fn note_names_parse() {
        let naturals = [
            ("C", 0),
            ("D", 2),
            ("E", 4),
            ("F", 5),
            ("G", 7),
            ("A", 9),
            ("B", 11),
        ];
        for (name, semitone) in naturals {
            assert_eq!(semitone_from_name(name), Some(semitone), "{name}");
            assert_eq!(semitone_from_name(&name.to_lowercase()), Some(semitone));
        }

        // Sharps and flats, in every spelling.
        assert_eq!(semitone_from_name("C#"), Some(1));
        assert_eq!(semitone_from_name("Cs"), Some(1));
        assert_eq!(semitone_from_name("cs"), Some(1));
        assert_eq!(semitone_from_name("Db"), Some(1));
        assert_eq!(semitone_from_name("db"), Some(1));
        assert_eq!(semitone_from_name("Eb"), Some(3));
        assert_eq!(semitone_from_name("D#"), Some(3));
        assert_eq!(semitone_from_name("Ab"), Some(8));
        assert_eq!(semitone_from_name("G#"), Some(8));

        // Enharmonics across the octave line wrap rather than going negative.
        assert_eq!(semitone_from_name("Cb"), Some(11));
        assert_eq!(semitone_from_name("B#"), Some(0));

        // Double accidentals fall out of the same loop.
        assert_eq!(semitone_from_name("Fbb"), Some(3));
        assert_eq!(semitone_from_name("C##"), Some(2));

        assert_eq!(semitone_from_name("H"), None);
        assert_eq!(semitone_from_name(""), None);
        assert_eq!(semitone_from_name("C4"), None); // octave numbers are not pitch classes
        assert_eq!(semitone_from_name("Cx"), None);
    }

    #[test]
    fn notes_param_accepts_names_and_semitones() {
        let expected = C | E | (1 << 7); // C E G

        let names = [
            Value::Ident("C".to_string()),
            Value::Ident("E".to_string()),
            Value::Ident("G".to_string()),
        ];
        // Octave-shifted, quoted and mixed forms land on the same three classes.
        let semitones = [Value::U32(0), Value::U32(4), Value::U32(7)];
        let mixed = [
            Value::Ident("c".to_string()),
            Value::U32(16),
            Value::String("G".to_string()),
        ];
        let negative = [Value::I32(-12), Value::I32(-8), Value::I32(-5)];

        assert_eq!(mask_from_notes(&names).unwrap(), expected);
        assert_eq!(mask_from_notes(&semitones).unwrap(), expected);
        assert_eq!(mask_from_notes(&mixed).unwrap(), expected);
        assert_eq!(mask_from_notes(&negative).unwrap(), expected);

        // A quoted sharp and its `s` spelling agree.
        assert_eq!(
            mask_from_notes(&[Value::String("F#".to_string())]).unwrap(),
            mask_from_notes(&[Value::Ident("Fs".to_string())]).unwrap()
        );

        assert_eq!(mask_from_notes(&[]).unwrap(), 0);
        assert!(mask_from_notes(&[Value::Ident("H".to_string())]).is_err());
        assert!(mask_from_notes(&[Value::Bool(true)]).is_err());
    }

    // ── The pitch/frequency convention ──────────────────────────────────────

    /// The anchor: MIDI 69 is A4 is 440 Hz, and an octave is a doubling.
    #[test]
    fn midi_note_numbers_line_up_with_frequency() {
        assert_eq!(mtof_f32(69.0), 440.0);
        assert_eq!(mtof_f32(81.0), 880.0);
        assert!((mtof_f32(57.0) - 220.0).abs() < 1e-3);
        assert!((mtof_f32(60.0) - 261.6256).abs() < 1e-3); // middle C, C4
        assert!((mtof_f32(21.0) - 27.5).abs() < 1e-4); // A0, bottom of a piano
        assert!((mtof_f32(108.0) - 4186.009).abs() < 1e-2); // C8, the top

        // Note 0 is C-1, not A0 — that is what makes `note % 12 == 0` a C, and
        // therefore what lets bit 0 of the mask mean C.
        assert_eq!(semitone_from_name("C"), Some(0));
        assert_eq!(60_i32.rem_euclid(12), semitone_from_name("C").unwrap());
        assert_eq!(69_i32.rem_euclid(12), semitone_from_name("A").unwrap());
    }

    /// The float path agrees with the `u8` one wherever both are defined, so a
    /// quantized note and a MIDI note-on of the same number are the same pitch.
    #[test]
    fn mtof_f32_agrees_with_midi_note_ons() {
        for note in 0..=127u8 {
            assert_eq!(mtof(note), mtof_f32(note as f32));
        }
    }

    #[test]
    fn ftom_inverts_mtof() {
        for note in -24..=140 {
            let note = note as f32;
            assert!(
                (ftom(mtof_f32(note)) - note).abs() < 1e-3,
                "round trip failed at {note}"
            );
        }
        assert_eq!(ftom(0.0), f32::NEG_INFINITY);
        assert_eq!(ftom(-1.0), f32::NEG_INFINITY);
    }

    /// Frequency is continuous and strictly increasing in pitch: fractional
    /// notes are real pitches, not rounding artifacts.
    #[test]
    fn frequency_is_continuous_in_pitch() {
        let quarter_tone = mtof_f32(69.5);
        assert!(quarter_tone > 440.0 && quarter_tone < mtof_f32(70.0));
        // Halfway in pitch is the geometric mean in Hz.
        assert!((quarter_tone - (440.0f32 * mtof_f32(70.0)).sqrt()).abs() < 1e-2);

        let mut prev = 0.0;
        for step in 0..1400 {
            let freq = mtof_f32(step as f32 / 10.0);
            assert!(freq > prev);
            prev = freq;
        }
    }

    #[test]
    fn quantize_to_freq_is_mtof_of_the_quantized_note() {
        let q = Quantizer::new(C_MAJOR).unwrap();
        for step in 0..1200 {
            let pitch = step as f32 / 10.0;
            assert_eq!(q.quantize_to_freq(pitch), mtof_f32(q.quantize(pitch)));
        }
        // 68.6 sits between G#4 and A4; A4 is in C major, and is 440 Hz exactly.
        assert_eq!(q.quantize(68.6), 69.0);
        assert_eq!(q.quantize_to_freq(68.6), 440.0);
    }

    // ── Node behavior ───────────────────────────────────────────────────────

    #[test]
    fn node_outputs_freq_and_note() {
        let mut node = Quantize::new(Quantizer::new(C_MAJOR).unwrap());
        let mut out = [0.0f32; 3];

        // The first frame always lands on a new note, so `gate` fires.
        PerSampleNode::tick(&mut node, &[Some(68.6)], &mut out);
        assert_eq!(out, [440.0, 69.0, 1.0]);

        // Holding the same note holds `freq`/`note` and drops `gate` back down.
        PerSampleNode::tick(&mut node, &[Some(68.7)], &mut out);
        assert_eq!(out, [440.0, 69.0, 0.0]);

        // `freq` is the bare-`>>` default, so this patches into `sine`.
        assert_eq!(Node::ports(&node).default_out(), vec![0]);
    }

    #[test]
    fn block_and_tick_paths_agree() {
        use crate::{config::Config, harness::build_placeholder_context};

        let pitches: Vec<f32> = (0..256).map(|i| 36.0 + i as f32 * 0.25).collect();

        let mut node = Quantize::new(Quantizer::new(C_MAJOR).unwrap());
        let mut ctx = build_placeholder_context(Config {
            block_size: 256,
            channels: 1,
            rt_capacity: 0,
            sample_rate: 48_000,
        });

        let mut freq = [0.0f32; 256];
        let mut note = [0.0f32; 256];
        let mut gate = [0.0f32; 256];
        node.process(
            &mut ctx,
            &[Some(&pitches)],
            &mut [&mut freq, &mut note, &mut gate],
        );

        let mut tick_node = Quantize::new(Quantizer::new(C_MAJOR).unwrap());
        let mut frame = [0.0f32; 3];
        for i in 0..256 {
            PerSampleNode::tick(&mut tick_node, &[Some(pitches[i])], &mut frame);
            assert_eq!(frame, [freq[i], note[i], gate[i]], "mismatch at sample {i}");
        }
    }

    #[test]
    fn unpatched_input_writes_nothing() {
        use crate::{config::Config, harness::build_placeholder_context};

        let mut node = Quantize::new(Quantizer::chromatic());
        let mut ctx = build_placeholder_context(Config {
            block_size: 4,
            channels: 1,
            rt_capacity: 0,
            sample_rate: 48_000,
        });

        let mut freq = [-1.0f32; 4];
        let mut note = [-1.0f32; 4];
        let mut gate = [-1.0f32; 4];
        node.process(&mut ctx, &[None], &mut [&mut freq, &mut note, &mut gate]);

        assert_eq!(freq, [-1.0; 4]);
        assert_eq!(note, [-1.0; 4]);
        assert_eq!(gate, [-1.0; 4]);
    }
}
