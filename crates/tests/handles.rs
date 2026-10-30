//! Nodes that own one end of a channel, with the application holding the other.
//!
//! The factory runs once per spawned instance, so every instance must get its
//! own pair, and the application must be able to find each one by path.

use std::sync::{Arc, Mutex};

use legato::{
    builder::{LegatoBuilder, Unconfigured, ValidationError},
    config::Config,
    handles::{ExportsHandle, HandleError},
    prelude::*,
};

const BLOCK: usize = 64;

fn config() -> Config {
    Config {
        sample_rate: 48_000,
        block_size: BLOCK,
        channels: 2,
        rt_capacity: 0,
    }
}

/// Outputs the latest value the application pushed. Not `Clone`: it owns a consumer.
struct Knob {
    rx: rtrb::Consumer<f32>,
    value: f32,
    ports: Ports,
}

impl Knob {
    fn new(rx: rtrb::Consumer<f32>) -> Self {
        Self {
            rx,
            value: 0.0,
            ports: PortBuilder::default().audio_out(1).build(),
        }
    }
}

impl Node for Knob {
    fn process(&mut self, _: &mut AudioContext, _: &Inputs, outputs: &mut [&mut [f32]]) {
        while let Ok(v) = self.rx.pop() {
            self.value = v;
        }
        outputs[0].fill(self.value);
    }
    fn ports(&self) -> &Ports {
        &self.ports
    }
}

impl NodeDefinition for Knob {
    const NAME: &'static str = "knob";
    const DESCRIPTION: &'static str = "Outputs a value pushed from the application";
    const REQUIRED_PARAMS: &'static [&'static str] = &[];
    const OPTIONAL_PARAMS: &'static [&'static str] = &["capacity"];

    fn create(
        rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        let capacity = p.get_usize("capacity")?.unwrap_or(16);
        let (tx, rx) = rtrb::RingBuffer::new(capacity);
        rb.export_handle(tx)?;
        Ok(Box::new(Knob::new(rx)))
    }
}

impl ExportsHandle for Knob {
    type Handle = rtrb::Producer<f32>;
}

fn build(src: &str) -> Result<(LegatoApp, LegatoFrontend), ValidationError> {
    LegatoBuilder::<Unconfigured>::new(config())
        .register_node("user", Knob::spec())
        .build_dsl(src)
}

fn first_sample(app: &mut LegatoApp) -> f32 {
    app.next_block().channels[0][0]
}

#[test]
fn each_spawned_instance_gets_its_own_channel() {
    let (mut app, mut frontend) = build(
        r#"
        user { knob * 3 }
        audio { track_mixer: mix { tracks: 3, chans_per_track: 1, gain: [1.0, 1.0, 1.0] } }
        knob(*) >> mix[0..3]
        { mix }
        "#,
    )
    .expect("graph should build");

    let mut knobs = frontend
        .take_handles_of::<Knob>("knob")
        .expect("three knob handles");

    let paths: Vec<&str> = knobs.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(paths, ["knob.0", "knob.1", "knob.2"]);

    // Only the middle instance hears this.
    knobs[1].1.push(10.0).unwrap();
    assert_eq!(first_sample(&mut app), 10.0);

    // If any two instances shared a consumer, the sum could not be 111.
    knobs[0].1.push(1.0).unwrap();
    knobs[2].1.push(100.0).unwrap();
    assert_eq!(first_sample(&mut app), 111.0);
}

#[test]
fn nested_patch_paths_are_outermost_first() {
    let (_app, mut frontend) = build(
        r#"
        patch voice {
            user { knob }
            { knob }
        }
        patches { voice * 2 }
        audio { track_mixer: mix { tracks: 2, chans_per_track: 1 } }
        voice(*) >> mix[0..2]
        { mix }
        "#,
    )
    .expect("graph should build");

    assert_eq!(frontend.handle_paths(), ["voice.0.knob", "voice.1.knob"]);

    let first = frontend.take_handle_of::<Knob>("voice.0.knob");
    assert!(first.is_ok());
    let rest = frontend.take_handles_of::<Knob>("voice").unwrap();
    assert_eq!(rest.len(), 1);
    assert_eq!(rest[0].0, "voice.1.knob");
}

#[test]
fn wrong_handle_type_is_an_error_and_keeps_the_handle() {
    let (_app, mut frontend) = build("user { knob }\n{ knob }").unwrap();

    let err = frontend
        .take_handle::<rtrb::Producer<u32>>("knob")
        .unwrap_err();
    assert!(matches!(err, HandleError::TypeMismatch { .. }), "{err:?}");

    assert!(frontend.take_handle::<rtrb::Producer<f32>>("knob").is_ok());
}

#[test]
fn missing_path_lists_what_is_available() {
    let (_app, mut frontend) = build("user { knob: cutoff }\n{ cutoff }").unwrap();

    let err = frontend.take_handle_of::<Knob>("cutof").unwrap_err();
    assert_eq!(
        err,
        HandleError::NotFound {
            path: "cutof".into(),
            available: vec!["cutoff".into()],
        }
    );
}

/// A second node type exporting the same handle type, to check kind matching.
struct OtherKnob;

impl NodeDefinition for OtherKnob {
    const NAME: &'static str = "other_knob";
    const DESCRIPTION: &'static str = "";
    const REQUIRED_PARAMS: &'static [&'static str] = &[];
    const OPTIONAL_PARAMS: &'static [&'static str] = &[];

    fn create(
        rb: &mut ResourceBuilderView,
        _: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        let (tx, rx) = rtrb::RingBuffer::new(4);
        rb.export_handle(tx)?;
        Ok(Box::new(Knob::new(rx)))
    }
}

#[test]
fn take_handle_of_checks_the_node_kind() {
    let (_app, mut frontend) = LegatoBuilder::<Unconfigured>::new(config())
        .register_node("user", OtherKnob::spec())
        .build_dsl("user { other_knob }\n{ other_knob }")
        .unwrap();

    let err = frontend.take_handle_of::<Knob>("other_knob").unwrap_err();
    assert!(
        matches!(&err, HandleError::WrongNodeKind { found, .. } if found == "other_knob"),
        "{err:?}"
    );
}

#[test]
fn exporting_twice_from_one_instance_fails_the_build() {
    let spec = NodeSpec::from_fn("greedy", &[], &[], |rb, _| {
        let (tx_a, rx) = rtrb::RingBuffer::<f32>::new(4);
        let (tx_b, _) = rtrb::RingBuffer::<f32>::new(4);
        rb.export_handle(tx_a)?;
        rb.export_handle(tx_b)?;
        Ok(Box::new(Knob::new(rx)))
    });

    let err = LegatoBuilder::<Unconfigured>::new(config())
        .register_node("user", spec)
        .build_dsl("user { greedy }\n{ greedy }")
        .map(|_| ())
        .unwrap_err();
    assert!(
        matches!(err, ValidationError::DuplicateHandle(_)),
        "{err:?}"
    );
}

/// The rare node that owns a single pre-built resource: the closure takes it
/// once, and a second spawn is a build error rather than a panic.
#[test]
fn single_resource_spawned_twice_is_a_build_error() {
    let (tx, rx) = rtrb::RingBuffer::<f32>::new(4);
    let slot = Arc::new(Mutex::new(Some(rx)));

    let spec = NodeSpec::from_fn("device", &[], &[], move |rb, _| {
        let rx = slot.lock().unwrap().take().ok_or_else(|| {
            ValidationError::InvalidParameter(format!(
                "'{}': device can only be spawned once",
                rb.instance_alias
            ))
        })?;
        Ok(Box::new(Knob::new(rx)))
    });

    let err = LegatoBuilder::<Unconfigured>::new(config())
        .register_node("user", spec)
        .build_dsl(
            r#"
            user { device * 2 }
            audio { track_mixer: mix { tracks: 2, chans_per_track: 1 } }
            device(*) >> mix[0..2]
            { mix }
            "#,
        )
        .map(|_| ())
        .unwrap_err();

    assert!(
        matches!(&err, ValidationError::InvalidParameter(m) if m.contains("device.1")),
        "{err:?}"
    );
    drop(tx);
}

#[test]
fn handles_and_nodes_resolve_a_selection_to_the_same_paths() {
    let src = r#"
        patch voice {
            user { knob, knob: knob2 }
            audio { track_mixer: sum { tracks: 2, chans_per_track: 1 } }
            knob >> sum[0]
            knob2 >> sum[1]
            { sum }
        }
        patches { voice * 3 }
        audio { track_mixer: mix { tracks: 3, chans_per_track: 1 } }
        voice(*) >> mix[0..3]
        { mix }
    "#;
    for selection in [
        "voice",
        "voice(*).knob",
        "voice(1..3)",
        "voice.2.knob2",
        "voice(0).knob",
    ] {
        let (_app, mut frontend) = build(src).unwrap();
        // Only knobs export handles, so drop the mixers from the node side.
        let nodes: Vec<String> = frontend
            .select(selection)
            .unwrap()
            .into_iter()
            .map(|(name, _)| name.to_string())
            .filter(|name| name.contains("knob"))
            .collect();
        let handles: Vec<String> = frontend
            .take_handles_of::<Knob>(selection)
            .unwrap()
            .into_iter()
            .map(|(path, _)| path)
            .collect();
        assert_eq!(nodes, handles, "{selection}");
    }
}
