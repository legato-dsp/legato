use legato::{
    LegatoApp,
    builder::{LegatoBuilder, Unconfigured},
    config::Config,
    ports::PortBuilder,
};

fn render(src: &str, blocks: usize) -> Vec<f32> {
    let config = Config {
        sample_rate: 44_100,
        block_size: 4096,
        channels: 1,
        rt_capacity: 0,
    };

    let ports = PortBuilder::default().audio_out(1).build();
    let (mut app, _frontend): (LegatoApp, _) = LegatoBuilder::<Unconfigured>::new(config, ports)
        .build_dsl(src)
        .expect("graph should build");

    let mut out = Vec::with_capacity(blocks * 4096);
    for _ in 0..blocks {
        out.extend_from_slice(app.next_block().channels[0]);
    }
    out
}

fn stats(name: &str, v: &[f32]) {
    let peak = v.iter().fold(0.0f32, |a, b| a.max(b.abs()));
    let nonzero = v.iter().filter(|x| **x != 0.0).count();
    let min = v.iter().cloned().fold(f32::INFINITY, f32::min);
    let max = v.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    println!("{name}: peak={peak:e} nonzero={nonzero}/{} min={min} max={max}", v.len());
}

const VOICE: &str = r#"
        patch voice(
            attack = 50.0,
            decay = 30.0,
            sustain = 0.3,
            release = 50.0
        ) {
            in freq gate

            audio {
                saw { chans: 1 },
                adsr { attack: $attack, decay: $decay, sustain: $sustain, release: $release, chans: 1 },
            }

            freq >> saw
            gate >> adsr.gate
            saw >> adsr[1]

            { adsr }
        }
"#;

#[test]
fn diag_example_as_written() {
    let src = format!(
        r#"{VOICE}
        patches {{ voice {{ }}, }}
        audio {{
            svf {{ chans: 1, cutoff: 5400.0, q: 0.4, type: "lowpass" }},
            noise,
        }}
        modular {{
            sample_and_hold {{ hold_time: 1500.0 }},
            quantize {{ notes: [0, 2, 3, 10, 5, 7] }}
        }}
        noise >> sample_and_hold >> quantize
        quantize.gate >> voice.gate
        quantize.freq >> voice.freq
        voice >> svf[0]
        {{ svf }}
    "#
    );
    stats("as-written", &render(&src, 20));
}

#[test]
fn diag_quantize_freq_tap() {
    let src = r#"
        audio { noise, mult { val: 1.0 } }
        modular {
            sample_and_hold { hold_time: 1500.0 },
            quantize { notes: [0, 2, 3, 10, 5, 7] }
        }
        noise >> sample_and_hold >> quantize
        quantize.freq >> mult
        { mult }
    "#;
    stats("quantize.freq", &render(src, 20));
}

#[test]
fn diag_quantize_gate_tap() {
    let src = r#"
        audio { noise, mult { val: 1.0 } }
        modular {
            sample_and_hold { hold_time: 1500.0 },
            quantize { notes: [0, 2, 3, 10, 5, 7] }
        }
        noise >> sample_and_hold >> quantize
        quantize.gate >> mult
        { mult }
    "#;
    let v = render(src, 20);
    stats("quantize.gate", &v);
    let ones: Vec<usize> = v
        .iter()
        .enumerate()
        .filter(|(_, x)| **x != 0.0)
        .map(|(i, _)| i)
        .take(20)
        .collect();
    println!("gate high at sample indices: {ones:?}");
}

#[test]
fn diag_with_map_and_sustained_gate() {
    let src = format!(
        r#"{VOICE}
        patches {{ voice {{ }}, }}
        audio {{
            svf {{ chans: 1, cutoff: 5400.0, q: 0.4, type: "lowpass" }},
            noise,
        }}
        control {{ map {{ range: [-1, 1], new_range: [48, 72] }} }}
        modular {{
            sample_and_hold {{ hold_time: 300.0 }},
            quantize {{ notes: [0, 2, 3, 10, 5, 7] }}
        }}
        noise >> map >> sample_and_hold >> quantize
        quantize.freq >> voice.freq
        voice >> svf[0]
        {{ svf }}
    "#
    );
    // gate left unpatched on purpose: see whether adsr panics or is silent
    stats("with-map-no-gate", &render(&src, 20));
}
