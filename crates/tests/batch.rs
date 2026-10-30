//! Batches: everything in one lands in the same block, and every batch comes
//! back to the frontend.

use legato::{
    FrontendError, LegatoApp, LegatoFrontend,
    builder::{LegatoBuilder, Unconfigured},
    config::Config,
    msg::LegatoMsg,
    node_state::NodeState::{self, *},
    resources::params::ParamError,
};
use proptest::prelude::*;

const CONFIG: Config = Config {
    sample_rate: 48_000,
    block_size: 64,
    channels: 1,
    rt_capacity: 0,
};

/// `saw >> svf`, with a shared `cut` param on the cutoff.
const SRC: &str = "control { signal: cut { name: \"cut\", min: 20.0, max: 20000.0, default: 1000.0 } }\n\
                   audio { saw { chans: 1 }, svf { chans: 1 } }\n\
                   saw >> svf\ncut >> svf.cutoff\n{ svf }";

fn build() -> (LegatoApp, LegatoFrontend) {
    LegatoBuilder::<Unconfigured>::new(CONFIG)
        .build_dsl(SRC)
        .unwrap()
}

fn state(app: &LegatoApp, frontend: &LegatoFrontend, name: &str) -> NodeState {
    app.node_state(frontend.clone_registry()[name]).unwrap()
}

#[test]
fn a_batch_lands_whole_in_the_next_block() {
    let (mut app, mut frontend) = build();
    let mut batch = frontend.batch();
    batch.set_param("cut", 500.0).unwrap();
    batch.bypass("svf").unwrap();
    batch.mute("saw").unwrap();
    let batch = batch.finish();
    frontend.send(&batch).unwrap();

    // Nothing is applied until the audio thread takes the batch, not even the
    // shared param, which `set_param` would write immediately.
    assert_eq!(frontend.get_param("cut"), Ok(1000.0));
    assert_eq!(state(&app, &frontend, "svf"), Active);

    app.next_block();
    assert_eq!(frontend.get_param("cut"), Ok(500.0));
    assert_eq!(state(&app, &frontend, "svf"), Bypassed);
    assert_eq!(state(&app, &frontend, "saw"), Muted);
}

#[test]
fn shared_params_are_clamped_when_the_batch_is_built() {
    let (mut app, mut frontend) = build();
    let mut batch = frontend.batch();
    batch.set_param("cut", 1e9).unwrap();
    assert_eq!(
        batch.finish().messages(),
        [LegatoMsg::SetParam(
            frontend.get_param_key("cut").unwrap(),
            20000.0
        )]
    );
    let mut batch = frontend.batch();
    batch.set_param("cut", 1e9).unwrap();
    let batch = batch.finish();
    frontend.send(&batch).unwrap();
    app.next_block();
    assert_eq!(frontend.get_param("cut"), Ok(20000.0));
}

#[test]
fn bad_entries_are_rejected_when_the_batch_is_built() {
    let (_app, frontend) = build();
    let mut batch = frontend.batch();
    assert_eq!(
        batch.mute("nope").err(),
        Some(FrontendError::NodeNotFound("nope".into()))
    );
    assert_eq!(
        batch.bypass("saw").err(),
        Some(FrontendError::CannotBypass {
            node: "saw".into(),
            inputs: 0,
            outputs: 1,
        })
    );
    assert_eq!(
        batch.set_param("nope", 1.0).err(),
        Some(FrontendError::Param(
            "nope".into(),
            ParamError::ParamNotFound
        ))
    );
    // Rejected entries leave nothing behind.
    assert!(batch.finish().messages().is_empty());
}

#[test]
fn a_full_return_ring_holds_back_the_batch_and_everything_after_it() {
    let (mut app, mut frontend) = build();
    let mut noop = frontend.batch();
    noop.activate("svf").unwrap();
    let noop = noop.finish();

    // `send_msg` skips the drain `send` does, so 512 returns fill the ring.
    for _ in 0..512 {
        frontend.send_msg(LegatoMsg::Batch(noop.clone())).unwrap();
    }
    app.next_block();

    let mut mute = frontend.batch();
    mute.mute("svf").unwrap();
    frontend.send_msg(LegatoMsg::Batch(mute.finish())).unwrap();
    frontend.set_state("saw", Suspended).unwrap();

    // Neither the batch nor the message queued behind it is applied.
    app.next_block();
    assert_eq!(state(&app, &frontend, "svf"), Active);
    assert_eq!(state(&app, &frontend, "saw"), Active);

    frontend.drain_garbage();
    app.next_block();
    assert_eq!(state(&app, &frontend, "svf"), Muted);
    assert_eq!(state(&app, &frontend, "saw"), Suspended);
}

fn any_state() -> impl Strategy<Value = NodeState> {
    prop_oneof![Just(Active), Just(Muted), Just(Bypassed), Just(Suspended)]
}

proptest! {
    /// A batch renders exactly like the same changes sent one by one before
    /// the same block, so the last state set for a node wins.
    #[test]
    fn a_batch_matches_its_messages_sent_one_by_one(
        changes in prop::collection::vec((any::<bool>(), any_state(), 1.0f32..30000.0), 1..16),
        blocks in 1usize..12,
    ) {
        let (mut batched, mut batched_fe) = build();
        let (mut single, mut single_fe) = build();

        let mut batch = batched_fe.batch();
        for &(on_svf, state, cut) in &changes {
            let node = if on_svf { "svf" } else { "saw" };
            // `saw` can't be bypassed; both sides skip it the same way.
            if !(node == "saw" && state == Bypassed) {
                batch.set_state(node, state).unwrap();
                single_fe.set_state(node, state).unwrap();
            }
            batch.set_param("cut", cut).unwrap();
            single_fe.set_param("cut", cut).unwrap();
        }
        let batch = batch.finish();
        batched_fe.send(&batch).unwrap();

        for _ in 0..blocks {
            let a = batched.next_block().channels[0].to_vec();
            let b = single.next_block().channels[0].to_vec();
            prop_assert_eq!(a, b);
        }
        for node in ["saw", "svf"] {
            prop_assert_eq!(state(&batched, &batched_fe, node), state(&single, &single_fe, node));
        }
    }
}
