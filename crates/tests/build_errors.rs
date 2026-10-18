//! Mistakes in a graph must come back from `build_dsl` as an `Err`, never a panic.

use legato::builder::{LegatoBuilder, Unconfigured, ValidationError};
use legato::config::Config;

fn build(src: &str) -> Result<(), ValidationError> {
    let config = Config {
        sample_rate: 48_000,
        block_size: 64,
        channels: 2,
        rt_capacity: 0,
    };
    LegatoBuilder::<Unconfigured>::new(config)
        .build_dsl(src)
        .map(|_| ())
}

#[test]
fn unknown_namespace() {
    let err = build("synths { sine {} }\n{ sine }").unwrap_err();
    assert!(
        matches!(err, ValidationError::NamespaceNotFound(_)),
        "{err:?}"
    );
}

#[test]
fn unknown_node() {
    let err = build("audio { sinee {} }\n{ sinee }").unwrap_err();
    assert!(matches!(err, ValidationError::NodeNotFound(_)), "{err:?}");
}

#[test]
fn unknown_param() {
    let err = build("audio { sine { frequency: 440.0 } }\n{ sine }").unwrap_err();
    assert!(
        matches!(err, ValidationError::InvalidParameter(_)),
        "{err:?}"
    );
}

#[test]
fn wrongly_typed_param() {
    let err = build("audio { saw { chans: 2.0 } }\n{ saw }").unwrap_err();
    assert!(
        matches!(err, ValidationError::InvalidParameter(_)),
        "{err:?}"
    );
}

#[test]
fn missing_required_param() {
    let err = build("audio { saw {} }\n{ saw }").unwrap_err();
    assert!(
        matches!(err, ValidationError::MissingRequiredParameter(_)),
        "{err:?}"
    );
}

#[test]
fn unknown_named_port() {
    let src = "audio { sine, svf { chans: 1 } }\nsine >> svf.resonance\n{ svf }";
    let err = build(src).unwrap_err();
    assert!(
        matches!(err, ValidationError::InvalidConnection(_)),
        "{err:?}"
    );
}

#[test]
fn mismatched_port_counts() {
    let src = "audio { saw { chans: 3 }, svf { chans: 2 } }\nsaw[0..3] >> svf[0..2]\n{ svf }";
    let err = build(src).unwrap_err();
    assert!(matches!(err, ValidationError::SelectionArity(_)), "{err:?}");
}

#[test]
fn zero_stride() {
    let src = "audio { saw { chans: 4 }, svf { chans: 1 } }\nsaw[0:4:0] >> svf\n{ svf }";
    let err = build(src).unwrap_err();
    assert!(
        matches!(err, ValidationError::InvalidConnection(_)),
        "{err:?}"
    );
}

#[test]
fn undeclared_alias() {
    let err = build("audio { sine }\nsaw >> sine\n{ sine }").unwrap_err();
    assert!(matches!(err, ValidationError::NodeNotFound(_)), "{err:?}");
}

#[test]
fn required_input_left_unpatched() {
    let err = build("control { sequencer { num_steps: 4 } }\n{ sequencer }").unwrap_err();
    assert!(
        matches!(err, ValidationError::UnconnectedInput(_)),
        "{err:?}"
    );
}

#[test]
fn ops_val_is_optional() {
    build("audio { sine, mult }\nsine >> mult\n{ mult }").expect("mult without val");
}

#[test]
fn multichannel_sine_is_rejected() {
    let err = build("audio { sine { chans: 2 } }\n{ sine }").unwrap_err();
    assert!(
        matches!(err, ValidationError::InvalidParameter(_)),
        "{err:?}"
    );
}

#[test]
fn constant_and_fractional_bpm_build() {
    build("control { constant { val: 0.5, chans: 2 } }\n{ constant }").expect("constant");
    let src = "control { clock { bpm: 92.5, division: 4, steps: 16 }, sequencer { num_steps: 16 } }\n\
               clock >> sequencer\n{ sequencer }";
    build(src).expect("fractional bpm");
}
