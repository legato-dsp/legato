//! Nodes clear their state when bypassed, so they resume clean.

use legato::{
    LegatoApp, LegatoFrontend,
    builder::{LegatoBuilder, Unconfigured},
    config::Config,
    node_state::NodeState,
};

const BLOCK: usize = 64;
/// Comfortably past the 5 ms fade at 48 kHz.
const SETTLE: usize = 10;

fn build(src: &str) -> (LegatoApp, LegatoFrontend) {
    LegatoBuilder::<Unconfigured>::new(Config {
        sample_rate: 48_000,
        block_size: BLOCK,
        channels: 1,
        rt_capacity: 0,
    })
    .build_dsl(src)
    .unwrap()
}

fn render(app: &mut LegatoApp, blocks: usize) -> Vec<f32> {
    (0..blocks)
        .flat_map(|_| app.next_block().channels[0].to_vec())
        .collect()
}

#[test]
fn bypassing_a_delay_write_clears_its_line() {
    let (mut app, mut frontend) = build(
        r#"
        control { constant: src { val: 1.0, chans: 1 } }
        audio {
            delay_write: dw { delay_name: "d", delay_length: 100.0, chans: 1 },
            delay_read: dr { delay_name: "d", delay_length: 20.0, chans: 1 }
        }
        src >> dw
        { dr }
        "#,
    );
    assert!(render(&mut app, 40).iter().any(|&y| y != 0.0));

    // Silence the source and bypass the writer, so nothing new reaches the
    // line; only the reset can empty it.
    frontend.set_state("src", NodeState::Suspended).unwrap();
    frontend.set_state("dw", NodeState::Bypassed).unwrap();
    render(&mut app, SETTLE);
    frontend.set_state("dw", NodeState::Active).unwrap();

    assert!(render(&mut app, 40).iter().all(|&y| y == 0.0));
}

#[test]
fn bypassing_a_filter_clears_its_memory() {
    let (mut app, mut frontend) = build(
        r#"
        control { constant: src { val: 1.0, chans: 1 } }
        audio {
            onepole: lp { cutoff: 5.0, chans: 1 }
        }
        src >> lp
        { lp }
        "#,
    );
    render(&mut app, 40);

    frontend.set_state("src", NodeState::Suspended).unwrap();
    frontend.set_state("lp", NodeState::Bypassed).unwrap();
    render(&mut app, SETTLE);
    frontend.set_state("lp", NodeState::Active).unwrap();

    // A slow lowpass would ring for seconds on its old charge without a reset.
    let out = render(&mut app, 1);
    assert!(out.iter().all(|&y| y == 0.0), "{out:?}");
}

#[test]
fn suspending_a_delay_write_clears_its_line() {
    let (mut app, mut frontend) = build(
        r#"
        control { constant: src { val: 1.0, chans: 1 } }
        audio {
            delay_write: dw { delay_name: "d", delay_length: 100.0, chans: 1 },
            delay_read: dr { delay_name: "d", delay_length: 20.0, chans: 1 }
        }
        src >> dw
        { dr }
        "#,
    );
    assert!(render(&mut app, 40).iter().any(|&y| y != 0.0));

    frontend.set_state("src", NodeState::Suspended).unwrap();
    frontend.set_state("dw", NodeState::Suspended).unwrap();
    render(&mut app, SETTLE);
    frontend.set_state("dw", NodeState::Active).unwrap();

    assert!(render(&mut app, 40).iter().all(|&y| y == 0.0));
}
