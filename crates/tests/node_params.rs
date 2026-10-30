//! Node params are checked on the control thread: a name the node would
//! ignore is an error when sent or batched, not a silent no-op.

use legato::{
    FrontendError, LegatoFrontend,
    builder::{LegatoBuilder, Unconfigured},
    config::Config,
    msg::{NodeMessage, ParamPayload, RtValue},
    spec::NodeDefinition,
};

legato_macros::include_node!("kernels/modtap4.legato", "modtap4");

fn frontend(src: &str) -> LegatoFrontend {
    let config = Config {
        sample_rate: 48_000,
        block_size: 64,
        channels: 2,
        rt_capacity: 0,
    };
    LegatoBuilder::<Unconfigured>::new(config)
        .register_node("audio", Modtap4::spec())
        .build_dsl(src)
        .unwrap()
        .1
}

fn set(param: &'static str) -> NodeMessage {
    NodeMessage::SetParam(ParamPayload {
        param_name: param,
        value: RtValue::F32(1.0),
    })
}

/// `saw >> svf >> mono_fan_out`, with `svf.cutoff` patched from a signal.
const SRC: &str = "control { signal: cut { name: \"cut\", min: 20.0, max: 20000.0, default: 1000.0 } }\n\
                   audio { saw { chans: 1 }, svf { chans: 1 }, mono_fan_out { chans: 2 } }\n\
                   saw >> svf >> mono_fan_out\ncut >> svf.cutoff\n{ mono_fan_out }";

#[test]
fn a_misspelled_param_is_rejected_with_the_names_the_node_takes() {
    let mut fe = frontend(SRC);
    let err = fe.send_node_msg("svf", set("Q")).unwrap_err();
    assert_eq!(
        err,
        FrontendError::UnknownNodeParam {
            node: "svf".into(),
            param: "Q".into(),
            accepted: vec!["cutoff".into(), "q".into(), "gain".into()],
        }
    );
    assert_eq!(
        err.to_string(),
        "'svf' has no runtime param 'Q' (accepts: cutoff, q, gain)"
    );
    // The batch builder applies the same check.
    assert_eq!(fe.batch().set("svf", "Q", 1.0).err(), Some(err));
    assert_eq!(fe.send_node_msg("svf", set("q")), Ok(()));
}

#[test]
fn a_node_without_runtime_params_rejects_every_name() {
    let mut fe = frontend(SRC);
    assert_eq!(
        fe.send_node_msg("mono_fan_out", set("chans"))
            .unwrap_err()
            .to_string(),
        "'mono_fan_out' has no runtime param 'chans' (accepts: none)"
    );
}

#[test]
fn a_param_driven_by_a_patched_input_is_rejected() {
    let mut fe = frontend(SRC);
    let err = fe.batch().set("svf", "cutoff", 500.0).err().unwrap();
    assert_eq!(
        err,
        FrontendError::PatchedParam {
            node: "svf".into(),
            param: "cutoff".into(),
        }
    );
    assert_eq!(
        err.to_string(),
        "'svf.cutoff' is driven by its patched input, so setting it has no effect; \
         change what feeds it instead"
    );
    // Unpatched params of the same node are still settable.
    assert!(fe.batch().set("svf", "q", 0.5).is_ok());
}

#[test]
fn kernels_take_their_declared_params() {
    let mut fe = frontend(
        "kernel scaled(amount = 0.5) {\n\
             in audio_in\n\
             audio { mult { val: $amount } }\n\
             audio_in >> mult[0]\n\
             { mult }\n\
         }\n\
         audio { saw { chans: 1 } }\n\
         patches { scaled: s {} }\n\
         saw >> s.audio_in\n{ s }",
    );
    assert_eq!(fe.send_node_msg("s", set("amount")), Ok(()));
    assert_eq!(
        fe.send_node_msg("s", set("amout")),
        Err(FrontendError::UnknownNodeParam {
            node: "s".into(),
            param: "amout".into(),
            accepted: vec!["amount".into()],
        })
    );
}

#[test]
fn generated_kernels_take_their_declared_params() {
    let mut fe = frontend("audio { saw { chans: 1 }, modtap4 }\nsaw >> modtap4\n{ modtap4 }");
    for param in ["depth", "feedback", "rate"] {
        assert_eq!(fe.send_node_msg("modtap4", set(param)), Ok(()), "{param}");
    }
    assert!(matches!(
        fe.send_node_msg("modtap4", set("dept")),
        Err(FrontendError::UnknownNodeParam { .. })
    ));
}
