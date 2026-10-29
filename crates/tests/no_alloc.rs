//! The audio thread must never touch the allocator, including to free
//! buffers the frontend has replaced. The intent here is that we will
//! drop unused resources on the UI/control thread.
//!
//! This binary installs [`AllocDisabler`] as its global allocator;
//! `warn_debug` gives countable allocations instead of panicking.

use std::{fs, path::PathBuf};

use assert_no_alloc::{AllocDisabler, assert_no_alloc, violation_count};
use legato::{
    LegatoApp, LegatoFrontend,
    builder::{LegatoBuilder, Unconfigured},
    config::Config,
    msg::{NodeMessage, ParamPayload, RtValue},
    node_state::NodeState,
    resources::buffer::ExternalBuffer,
    spec::NodeDefinition,
};

#[global_allocator]
static A: AllocDisabler = AllocDisabler;

legato_macros::include_node!("kernels/modtap4.legato", "modtap4");

const BLOCKS: usize = 32;

/// Allocator calls made by `f`. Counters are thread-local, so parallel tests
/// do not interfere.
fn allocs_in(f: impl FnOnce()) -> u32 {
    let before = violation_count();
    assert_no_alloc(f);
    violation_count() - before
}

/// Allocator calls made while rendering one block.
fn render_allocs(app: &mut LegatoApp) -> u32 {
    allocs_in(|| {
        app.next_block();
    })
}

fn build(src: &str) -> (LegatoApp, LegatoFrontend) {
    let config = Config {
        sample_rate: 44_100,
        block_size: 256,
        channels: 2,
        rt_capacity: 0,
    };
    LegatoBuilder::<Unconfigured>::new(config)
        .register_node("audio", Modtap4::spec())
        .build_dsl(src)
        .expect("graph should build")
}

/// A stereo buffer of `len` frames, allocated on the calling (frontend) thread.
fn buffer(len: usize) -> ExternalBuffer {
    ExternalBuffer {
        data: (0..len * 2).map(|x| x as f32).collect(),
        num_channels: 2,
    }
}

// We also want to catch deallocations on the audio thread
#[test]
fn harness_counts_a_free() {
    let v = vec![0u8; 16];
    assert_eq!(allocs_in(|| drop(v)), 1);
}

#[test]
fn demos_render_without_allocating() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("demos");
    let mut checked = 0;
    for entry in fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("legato") {
            continue;
        }
        let (mut app, _frontend) = build(&fs::read_to_string(&path).unwrap());
        // Includes the first block, so lazy setup on the audio thread is caught too.
        for block in 0..BLOCKS {
            assert_eq!(render_allocs(&mut app), 0, "{path:?} block {block}");
        }
        checked += 1;
    }
    assert!(checked > 0, "no demos found in {dir:?}");
}

const SAMPLER: &str = "audio { sampler { sampler_name: \"s\" } }\n{ sampler }";

#[test]
fn swapped_buffers_are_freed_on_the_frontend() {
    let (mut app, mut frontend) = build(SAMPLER);

    // Each block swaps in a new buffer; the replaced one holds the only reference
    // to its data, so dropping it on the audio thread would count a free.
    // 1100 swaps cycles the 512-slot garbage queue twice.
    for i in 0..1100 {
        frontend.send_buffer("s", buffer(64 + i)).unwrap();
        assert_eq!(render_allocs(&mut app), 0, "swap {i}");
    }
    // Freed here, outside the check.
    frontend.drain_garbage();
}

#[test]
fn params_and_node_messages_do_not_allocate() {
    let src = "control { signal: cut { name: \"cut\", min: 20.0, max: 20000.0, default: 1000.0 } }\n\
               audio { saw { chans: 1 }, svf { chans: 1 } }\n\
               saw >> svf\ncut >> svf.cutoff\n{ svf }";
    let (mut app, mut frontend) = build(src);

    // Change a shared param and send a node message before every block.
    for i in 0..BLOCKS {
        let f = 100.0 + i as f32 * 100.0;
        frontend.set_param("cut", f).unwrap();
        frontend
            .send_node_msg(
                "svf",
                NodeMessage::SetParam(ParamPayload {
                    param_name: "q",
                    value: RtValue::F32(0.5 + i as f32 * 0.1),
                }),
            )
            .unwrap();
        assert_eq!(render_allocs(&mut app), 0, "block {i}");
    }
}

#[test]
fn node_state_changes_do_not_allocate() {
    let src = "audio { saw { chans: 1 }, svf { chans: 1 } }\nsaw >> svf\n{ svf }";
    let (mut app, mut frontend) = build(src);
    use NodeState::*;
    let svf_states = [Muted, Bypassed, Suspended, Active];
    // `saw` marks no signal input, so it can't be bypassed.
    let saw_states = [Suspended, Active, Muted];

    // Change both nodes' states every 3 blocks, so some land mid-fade.
    for i in 0..BLOCKS * 4 {
        if i % 3 == 0 {
            let n = i / 3;
            frontend.set_state("svf", svf_states[n % 4]).unwrap();
            frontend.set_state("saw", saw_states[n % 3]).unwrap();
        }
        assert_eq!(render_allocs(&mut app), 0, "block {i}");
    }
}
