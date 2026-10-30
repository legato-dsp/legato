//! `signal` glides to each new value over `glide` ms.

use legato::{
    LegatoApp, LegatoFrontend,
    builder::{LegatoBuilder, Unconfigured, ValidationError},
    config::Config,
};

const SR: usize = 48_000;

fn build(extra: &str) -> Result<(LegatoApp, LegatoFrontend), ValidationError> {
    let config = Config {
        sample_rate: SR,
        block_size: 256,
        channels: 1,
        rt_capacity: 0,
    };
    let src = format!(
        "control {{ signal: s {{ name: \"s\", min: 0.0, max: 1.0, default: 0.0{extra} }} }}\n{{ s }}"
    );
    LegatoBuilder::<Unconfigured>::new(config).build_dsl(&src)
}

/// The two blocks after the param steps from 0 to 1.
fn step(extra: &str) -> Vec<f32> {
    let (mut app, mut frontend) = build(extra).unwrap();
    app.next_block();
    frontend.set_param("s", 1.0).unwrap();
    (0..2)
        .flat_map(|_| app.next_block().channels[0].to_vec())
        .collect()
}

#[test]
fn default_glide_covers_one_time_constant_in_five_ms() {
    let out = step("");
    // 5 ms at 48 kHz is 240 samples; a one-pole is 1 - 1/e of the way there.
    let at = |ms: f64| out[(ms * SR as f64 / 1000.0) as usize - 1] as f64;
    assert!(
        (at(5.0) - (1.0 - (-1.0f64).exp())).abs() < 1e-4,
        "{}",
        at(5.0)
    );
    assert!(out.windows(2).all(|w| w[0] <= w[1]), "rises monotonically");
}

#[test]
fn zero_glide_steps_immediately() {
    assert_eq!(step(", glide: 0.0"), vec![1.0; 512]);
}

#[test]
fn glide_scales_with_its_time() {
    // Twice the glide takes twice as many samples to reach the same point.
    let (short, long) = (step(", glide: 5.0"), step(", glide: 10.0"));
    assert!((short[239] - long[479]).abs() < 1e-4);
}

#[test]
fn the_old_smoothing_coefficient_is_rejected() {
    assert!(build(", smoothing: 0.5").is_err());
}
