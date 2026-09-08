use legato::{
    LegatoApp,
    builder::{LegatoBuilder, Unconfigured},
    config::Config,
    nodes::midi::voice::mtof_f32,
    ports::PortBuilder,
};

/// A minor pentatonic, as pitch classes: A C D E G.
const A_MINOR_PENTATONIC: [i32; 5] = [9, 0, 2, 4, 7];

/// The pitch range `map` is asked for below: C3 to C5.
const LO: f32 = 48.0;
const HI: f32 = 72.0;

fn render(src: &str, blocks: usize) -> Vec<f32> {
    let config = Config {
        sample_rate: 48_000,
        block_size: 256,
        channels: 1,
        rt_capacity: 0,
    };

    let ports = PortBuilder::default().audio_out(1).build();
    let (mut app, _frontend): (LegatoApp, _) = LegatoBuilder::<Unconfigured>::new(config, ports)
        .build_dsl(src)
        .expect("graph should build");

    let mut out = Vec::with_capacity(blocks * 256);
    for _ in 0..blocks {
        out.extend_from_slice(app.next_block().channels[0]);
    }
    out
}

/// The full patch: the quantizer's default output is Hz, so it drives `sine`
/// with no adapter in between.
#[test]
fn quantizer_drives_an_oscillator_directly() {
    let out = render(
        r#"
        audio { noise, sine { chans: 1 } }
        control { map { range: [-1, 1], new_range: [48, 72] } }
        modular {
            sample_and_hold { hold_time: 100 },
            quantize { notes: [A, C, D, E, G] }
        }

        noise >> map >> sample_and_hold >> quantize >> sine

        { sine }
    "#,
        40,
    );

    assert!(out.iter().any(|x| *x != 0.0), "patch rendered silence");
    assert!(
        out.iter().all(|x| x.is_finite() && x.abs() <= 1.01),
        "oscillator left its range, so the frequency it was handed was wrong"
    );
}

/// Tap `note` to read the pitches the chain actually produces. Every one must
/// be a whole semitone, in the scale, and inside the range `map` was given —
/// which together is the claim that the units survive the whole chain.
#[test]
fn quantized_pitches_stay_in_scale_and_in_range() {
    let notes = render(
        r#"
        audio { noise, mult { val: 1.0 } }
        control { map { range: [-1, 1], new_range: [48, 72] } }
        modular {
            sample_and_hold { hold_time: 20 },
            quantize { notes: [A, C, D, E, G] }
        }

        noise >> map >> sample_and_hold >> quantize
        quantize.note >> mult

        { mult }
    "#,
        40,
    );

    let mut seen = std::collections::BTreeSet::new();

    for note in &notes {
        assert_eq!(*note, note.round(), "not a whole semitone: {note}");
        assert!(
            (LO..=HI).contains(note),
            "{note} escaped the mapped range {LO}..{HI}"
        );

        let pitch_class = (*note as i32).rem_euclid(12);
        assert!(
            A_MINOR_PENTATONIC.contains(&pitch_class),
            "{note} (pitch class {pitch_class}) is out of scale"
        );

        seen.insert(*note as i32);
    }

    // Five pitch classes across two octaves; a stuck chain would show one or two.
    assert!(
        seen.len() > 5,
        "chain produced too little variety: {seen:?}"
    );
}

/// `freq` and `note` are two views of the same pitch, so `freq` must be
/// `mtof_f32(note)` sample for sample — the property that lets you patch either
/// one depending on what the downstream node wants.
#[test]
fn freq_and_note_outputs_agree() {
    const PATCH: &str = r#"
        audio {
            noise,
            mult { val: 1.0 }
        }

        control {
            map { range: [-1, 1], new_range: [48, 72] }
        }

        modular {
            sample_and_hold { hold_time: 20 },
            quantize { notes: [A, C, D, E, G] }
        }

        noise >> map >> sample_and_hold >> quantize
        quantize.PORT >> mult

        { mult }
    "#;

    // `noise` seeds itself from its plan identity, which is the same in both
    // graphs here, so the two renders see the same pitch stream.
    let freqs = render(&PATCH.replace("PORT", "freq"), 8);
    let notes = render(&PATCH.replace("PORT", "note"), 8);

    for (i, (freq, note)) in freqs.iter().zip(notes.iter()).enumerate() {
        assert_eq!(
            *freq,
            mtof_f32(*note),
            "sample {i}: {freq} Hz vs note {note}"
        );
    }
}

/// Semitone offsets and note names are the same scale written two ways.
#[test]
fn note_names_and_semitones_give_the_same_scale() {
    const PATCH: &str = r#"
        audio { noise, mult { val: 1.0 } }
        control { map { range: [-1, 1], new_range: [48, 72] } }
        modular {
            sample_and_hold { hold_time: 20 },
            quantize { notes: NOTES }
        }

        noise >> map >> sample_and_hold >> quantize
        quantize.note >> mult

        { mult }
    "#;

    let by_name = render(&PATCH.replace("NOTES", "[A, C, D, E, G]"), 8);
    let by_semitone = render(&PATCH.replace("NOTES", "[9, 0, 2, 4, 7]"), 8);

    assert_eq!(by_name, by_semitone);
}
