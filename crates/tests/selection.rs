//! Selections such as `voice(*).osc`: one resolver for batches, single-node
//! calls and handles.

use legato::{
    FrontendError, LegatoApp, LegatoFrontend,
    builder::{LegatoBuilder, Unconfigured},
    config::Config,
    dsl::ir::NodeSelector,
    node_state::NodeState::{self, *},
    select::Selection,
};
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Matching against a structural oracle
// ---------------------------------------------------------------------------

/// Names that share prefixes, so string-prefix bugs show up.
const NAMES: [&str; 4] = ["op", "op1", "op10", "v"];

/// A patch tree: a name, its instance count, and its children (none = leaf).
#[derive(Debug, Clone)]
struct Tree {
    name: &'static str,
    count: usize,
    children: Vec<Tree>,
}

fn tree() -> impl Strategy<Value = Vec<Tree>> {
    let leaf = (0..NAMES.len(), 1usize..4).prop_map(|(n, count)| Tree {
        name: NAMES[n],
        count,
        children: vec![],
    });
    let node = leaf.prop_recursive(3, 24, 3, |inner| {
        (
            0..NAMES.len(),
            1usize..4,
            prop::collection::vec(inner, 1..3),
        )
            .prop_map(|(n, count, children)| Tree {
                name: NAMES[n],
                count,
                children: dedupe(children),
            })
    });
    prop::collection::vec(node, 1..4).prop_map(dedupe)
}

/// Sibling names are unique, as DSL aliases are.
fn dedupe(mut trees: Vec<Tree>) -> Vec<Tree> {
    let mut seen = Vec::new();
    trees.retain(|t| {
        let fresh = !seen.contains(&t.name);
        seen.push(t.name);
        fresh
    });
    trees
}

/// Every leaf as its path string and its (name, instance) chain. Paths are
/// spelled the way the builder names nodes: an index segment only when a
/// declaration has more than one instance.
fn leaves(
    trees: &[Tree],
    prefix: &str,
    chain: &[(&'static str, usize)],
    out: &mut Vec<(String, Vec<(&'static str, usize)>)>,
) {
    for t in trees {
        for i in 0..t.count {
            let path = match (prefix.is_empty(), t.count > 1) {
                (true, true) => format!("{}.{i}", t.name),
                (true, false) => t.name.to_string(),
                (false, true) => format!("{prefix}.{}.{i}", t.name),
                (false, false) => format!("{prefix}.{}", t.name),
            };
            let mut chain = chain.to_vec();
            chain.push((t.name, i));
            if t.children.is_empty() {
                out.push((path, chain));
            } else {
                leaves(&t.children, &path, &chain, out);
            }
        }
    }
}

fn selector() -> impl Strategy<Value = NodeSelector> {
    prop_oneof![
        Just(NodeSelector::Single),
        Just(NodeSelector::All),
        (0usize..4).prop_map(NodeSelector::Index),
        (0usize..4, 0usize..4).prop_map(|(a, b)| NodeSelector::Range(a.min(b), a.max(b))),
    ]
}

/// A selection as segments, with an index sometimes written as `.i` rather
/// than `(i)`.
fn selection() -> impl Strategy<Value = Vec<(&'static str, NodeSelector, bool)>> {
    prop::collection::vec((0..NAMES.len(), selector(), any::<bool>()), 1..4).prop_map(|segs| {
        segs.into_iter()
            .map(|(n, s, dot)| (NAMES[n], s, dot))
            .collect()
    })
}

fn spell(segs: &[(&str, NodeSelector, bool)]) -> String {
    segs.iter()
        .map(|(name, sel, dot)| match sel {
            NodeSelector::Single => name.to_string(),
            NodeSelector::All => format!("{name}(*)"),
            NodeSelector::Index(i) if *dot => format!("{name}.{i}"),
            NodeSelector::Index(i) => format!("{name}({i})"),
            NodeSelector::Range(a, b) => format!("{name}({a}..{b})"),
        })
        .collect::<Vec<_>>()
        .join(".")
}

/// The oracle: compare segment by segment on the chain, never on strings.
fn oracle(segs: &[(&str, NodeSelector, bool)], chain: &[(&str, usize)]) -> bool {
    segs.len() <= chain.len()
        && segs.iter().zip(chain).all(|((name, sel, _), (n, i))| {
            name == n
                && match sel {
                    NodeSelector::Single | NodeSelector::All => true,
                    NodeSelector::Index(j) => i == j,
                    NodeSelector::Range(a, b) => (a..b).contains(&i),
                }
        })
}

proptest! {
    #[test]
    fn selections_match_whole_segments_and_instances(trees in tree(), segs in selection()) {
        let mut all = Vec::new();
        leaves(&trees, "", &[], &mut all);
        let spelled = spell(&segs);
        let selected = Selection::parse(&spelled).unwrap();
        for (path, chain) in &all {
            prop_assert_eq!(
                selected.matches(path),
                oracle(&segs, chain),
                "{} vs {}", spelled, path
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Through the frontend
// ---------------------------------------------------------------------------

const SRC: &str = "
    patch voice() {
        audio { sine: osc { freq: 220.0 }, mult: amp { val: 0.5 } }
        osc >> amp[0]
        { amp }
    }
    patch outer() {
        audio { voice: v {}, mult: gain { val: 1.0 } }
        v >> gain[0]
        { gain }
    }
    patches { voice * 3 {}, voice: lone {}, outer: o {} }
    audio { track_mixer: mix { tracks: 5, chans_per_track: 1, gain: [1.0, 1.0, 1.0, 1.0, 1.0] } }
    voice(*) >> mix[0..3]
    lone >> mix[3]
    o >> mix[4]
    { mix }
";

fn build() -> (LegatoApp, LegatoFrontend) {
    let config = Config {
        sample_rate: 48_000,
        block_size: 64,
        channels: 1,
        rt_capacity: 0,
    };
    LegatoBuilder::<Unconfigured>::new(config)
        .build_dsl(SRC)
        .unwrap()
}

/// Every node that isn't Active after sending `batch`, by name.
fn changed(f: impl FnOnce(&LegatoFrontend) -> legato::batch::Batch) -> Vec<(String, NodeState)> {
    let (mut app, mut frontend) = build();
    let batch = f(&frontend);
    frontend.send(&batch).unwrap();
    app.next_block();
    let mut out: Vec<(String, NodeState)> = frontend
        .clone_registry()
        .into_iter()
        .map(|(name, key)| (name, app.node_state(key).unwrap()))
        .filter(|(_, s)| *s != Active)
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn named(pairs: &[(&str, NodeState)]) -> Vec<(String, NodeState)> {
    pairs.iter().map(|&(n, s)| (n.to_string(), s)).collect()
}

#[test]
fn a_batch_applies_to_every_selected_node() {
    let got = changed(|fe| {
        let mut b = fe.batch();
        b.mute("voice(*).osc").unwrap();
        b.bypass("voice(1).amp").unwrap();
        b.suspend("o.v").unwrap();
        b.finish()
    });
    assert_eq!(
        got,
        named(&[
            ("o.v.amp", Suspended),
            ("o.v.osc", Suspended),
            ("voice.0.osc", Muted),
            ("voice.1.amp", Bypassed),
            ("voice.1.osc", Muted),
            ("voice.2.osc", Muted),
        ])
    );
}

#[test]
fn a_bare_instance_name_selects_everything_under_it() {
    let got = changed(|fe| {
        let mut b = fe.batch();
        b.mute("voice(0..2)").unwrap();
        b.finish()
    });
    assert_eq!(
        got,
        named(&[
            ("voice.0.amp", Muted),
            ("voice.0.osc", Muted),
            ("voice.1.amp", Muted),
            ("voice.1.osc", Muted),
        ])
    );
}

#[test]
fn a_failing_node_adds_nothing_from_that_call() {
    let (_app, frontend) = build();
    let mut b = frontend.batch();
    // Every `osc` is a sine, which can't be bypassed, so the whole call fails.
    assert_eq!(
        b.bypass("voice(*)").err(),
        Some(FrontendError::CannotBypass {
            node: "voice.0.osc".into(),
            inputs: 0,
            outputs: 1,
        })
    );
    assert!(b.finish().messages().is_empty());
}

#[test]
fn single_node_calls_need_exactly_one_match() {
    let (_app, mut frontend) = build();
    let err = frontend.set_state("voice(*).osc", Muted).unwrap_err();
    assert_eq!(
        err.to_string(),
        "'voice(*).osc' matches 3 nodes (voice.0.osc, voice.1.osc, voice.2.osc); \
         use a batch to change several"
    );
    assert_eq!(frontend.set_state("voice(2).osc", Muted), Ok(()));
    assert_eq!(
        frontend.set_state("voice(7)", Muted),
        Err(FrontendError::NodeNotFound("voice(7)".into()))
    );
    assert!(matches!(
        frontend.set_state("voice(", Muted),
        Err(FrontendError::InvalidSelection(_))
    ));
}
