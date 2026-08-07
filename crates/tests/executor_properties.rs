//! Property tests for the block-rate executor: DAG evaluation and arity validation.

use legato::{
    config::Config,
    context::AudioContext,
    executor::{Executor, MAX_ARITY},
    graph::{Connection, ConnectionEntry},
    harness::build_placeholder_context,
    node::{Inputs, LegatoNode, Node},
    ports::{PortBuilder, Ports},
    runtime::NodeKey,
};
use proptest::prelude::*;
use std::ops::{Range, RangeInclusive};

const SR: usize = 48_000;
const BLOCK: usize = 64;

/// Port counts per side for a generated node.
const PORTS: RangeInclusive<usize> = 1..=4;
/// Node count for a generated graph.
const NODES: Range<usize> = 2..8;
/// Candidate edges drawn per graph, before self-loops are filtered out.
const EDGES: Range<usize> = 0..12;

const GAIN: Range<f32> = -2.0..2.0;
const BIAS: Range<f32> = -1.0..1.0;

/// Relative tolerance: the executor and the reference sum fan-in in different orders.
const TOLERANCE: f32 = 1e-5;

/// How far past `MAX_ARITY` the arity property reaches.
const ARITY_OVERSHOOT: usize = 8;
/// Node count for the arity property.
const ARITY_NODES: Range<usize> = 1..5;

/// Stateless node: `out[p][s] = sum(inputs at s) * gain + bias * (p + 1)`.
#[derive(Clone)]
struct Affine {
    gain: f32,
    bias: f32,
    ports: Ports,
}

impl Affine {
    fn new(ins: usize, outs: usize, gain: f32, bias: f32) -> Self {
        Self {
            gain,
            bias,
            ports: PortBuilder::default().audio_in(ins).audio_out(outs).build(),
        }
    }

    /// The same arithmetic `process` performs.
    fn eval(&self, in_frames: &[Vec<f32>], out_port: usize, sample: usize) -> f32 {
        let sum: f32 = in_frames.iter().map(|chan| chan[sample]).sum();
        sum * self.gain + self.bias * (out_port + 1) as f32
    }
}

impl Node for Affine {
    fn process(&mut self, _: &mut AudioContext, inputs: &Inputs, outputs: &mut [&mut [f32]]) {
        let block = outputs.first().map_or(0, |o| o.len());

        for s in 0..block {
            let sum: f32 = inputs.iter().filter_map(|i| *i).map(|chan| chan[s]).sum();

            for (p, out) in outputs.iter_mut().enumerate() {
                out[s] = sum * self.gain + self.bias * (p + 1) as f32;
            }
        }
    }

    fn ports(&self) -> &Ports {
        &self.ports
    }
}

#[derive(Debug, Clone)]
struct NodeSpec {
    ins: usize,
    outs: usize,
    gain: f32,
    bias: f32,
}

/// An edge in declaration-index space; `src < snk` keeps the graph acyclic.
#[derive(Debug, Clone)]
struct EdgeSpec {
    src: usize,
    src_port: usize,
    snk: usize,
    snk_port: usize,
}

#[derive(Debug, Clone)]
struct GraphSpec {
    nodes: Vec<NodeSpec>,
    edges: Vec<EdgeSpec>,
}

fn node_spec() -> impl Strategy<Value = NodeSpec> {
    (PORTS, PORTS, GAIN, BIAS).prop_map(|(ins, outs, gain, bias)| NodeSpec {
        ins,
        outs,
        gain,
        bias,
    })
}

/// A random DAG: forward-only edges, port indices reduced into range.
fn graph_spec() -> impl Strategy<Value = GraphSpec> {
    prop::collection::vec(node_spec(), NODES).prop_flat_map(|nodes| {
        let n = nodes.len();
        let max_port = *PORTS.end();
        let edge = (0..n, 0..n, 0..max_port, 0..max_port);

        prop::collection::vec(edge, EDGES).prop_map(move |raw| {
            // The graph stores edges in an IndexSet, so an identical connection is
            // idempotent rather than additive; dedup to keep the spec representable.
            let mut seen = std::collections::HashSet::new();

            let edges = raw
                .iter()
                .filter_map(|&(a, b, src_port, snk_port)| {
                    let (src, snk) = match a.cmp(&b) {
                        std::cmp::Ordering::Less => (a, b),
                        std::cmp::Ordering::Greater => (b, a),
                        std::cmp::Ordering::Equal => return None,
                    };

                    let edge = EdgeSpec {
                        src,
                        src_port: src_port % nodes[src].outs,
                        snk,
                        snk_port: snk_port % nodes[snk].ins,
                    };

                    seen.insert((edge.src, edge.src_port, edge.snk, edge.snk_port))
                        .then_some(edge)
                })
                .collect();

            GraphSpec {
                nodes: nodes.clone(),
                edges,
            }
        })
    })
}

/// Build the executor, returning node keys in declaration order.
fn build(spec: &GraphSpec) -> (Executor, Vec<NodeKey>) {
    let mut executor = Executor::default();

    let keys: Vec<NodeKey> = spec
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            executor.graph.add_node(LegatoNode::new(
                format!("n{i}"),
                "Affine".into(),
                Box::new(Affine::new(n.ins, n.outs, n.gain, n.bias)),
            ))
        })
        .collect();

    for e in &spec.edges {
        executor
            .graph
            .add_edge(Connection {
                source: ConnectionEntry {
                    node_key: keys[e.src],
                    port_index: e.src_port,
                },
                sink: ConnectionEntry {
                    node_key: keys[e.snk],
                    port_index: e.snk_port,
                },
            })
            .expect("forward-only edges are acyclic");
    }

    (executor, keys)
}

/// Evaluate in declaration order, which is in topo order.
fn reference_eval(spec: &GraphSpec, block: usize) -> Vec<Vec<Vec<f32>>> {
    let mut outputs: Vec<Vec<Vec<f32>>> = Vec::with_capacity(spec.nodes.len());

    for (i, node) in spec.nodes.iter().enumerate() {
        let affine = Affine::new(node.ins, node.outs, node.gain, node.bias);

        let mut in_frames: Vec<Vec<f32>> = vec![vec![0.0; block]; node.ins];
        for e in spec.edges.iter().filter(|e| e.snk == i) {
            let upstream = &outputs[e.src][e.src_port];
            for s in 0..block {
                in_frames[e.snk_port][s] += upstream[s];
            }
        }

        outputs.push(
            (0..node.outs)
                .map(|p| (0..block).map(|s| affine.eval(&in_frames, p, s)).collect())
                .collect(),
        );
    }

    outputs
}

fn context() -> AudioContext {
    build_placeholder_context(Config {
        sample_rate: SR,
        block_size: BLOCK,
        channels: 2,
        rt_capacity: 0,
    })
}

fn close(got: f32, want: f32) -> bool {
    (got - want).abs() <= TOLERANCE * want.abs().max(1.0)
}

proptest! {
    /// P1: covers buffer offset arithmetic, fan-in accumulation and topo order.
    #[test]
    fn executor_matches_reference_evaluation(spec in graph_spec()) {
        let (mut executor, keys) = build(&spec);
        let expected = reference_eval(&spec, BLOCK);

        let mut ctx = context();

        // Check every node by making each the sink in turn.
        for (i, key) in keys.iter().enumerate() {
            executor.set_sink(*key).unwrap();
            executor.prepare(BLOCK);

            let view = executor.process(&mut ctx);

            prop_assert_eq!(
                view.chans,
                spec.nodes[i].outs,
                "node {} reported the wrong output arity",
                i
            );

            for port in 0..view.chans {
                for s in 0..BLOCK {
                    let (got, want) = (view.channels[port][s], expected[i][port][s]);
                    prop_assert!(
                        close(got, want),
                        "node {} port {} sample {}: executor gave {}, reference gave {}",
                        i, port, s, got, want
                    );
                }
            }
        }
    }

    /// P1 corollary: two sources into one sink port must sum.
    #[test]
    fn fan_in_is_additive(gain in GAIN, a in BIAS, b in BIAS) {
        let mut executor = Executor::default();

        let src_a = executor.graph.add_node(LegatoNode::new(
            "a".into(), "Affine".into(), Box::new(Affine::new(1, 1, 0.0, a)),
        ));
        let src_b = executor.graph.add_node(LegatoNode::new(
            "b".into(), "Affine".into(), Box::new(Affine::new(1, 1, 0.0, b)),
        ));
        let sink = executor.graph.add_node(LegatoNode::new(
            "sink".into(), "Affine".into(), Box::new(Affine::new(1, 1, gain, 0.0)),
        ));

        for src in [src_a, src_b] {
            executor.graph.add_edge(Connection {
                source: ConnectionEntry { node_key: src, port_index: 0 },
                sink: ConnectionEntry { node_key: sink, port_index: 0 },
            }).unwrap();
        }

        executor.set_sink(sink).unwrap();
        executor.prepare(BLOCK);

        let mut ctx = context();
        let view = executor.process(&mut ctx);

        let want = (a + b) * gain;
        for s in 0..BLOCK {
            prop_assert!(
                close(view.channels[0][s], want),
                "sample {}: got {}, expected sum of sources {}",
                s, view.channels[0][s], want
            );
        }
    }

    /// P2: validate_arity accepts iff every node fits, and accepted graphs must render.
    #[test]
    fn arity_validation_guards_the_executor(
        ports in prop::collection::vec(
            (1..=MAX_ARITY + ARITY_OVERSHOOT, 1..=MAX_ARITY + ARITY_OVERSHOOT),
            ARITY_NODES,
        ),
    ) {
        let mut executor = Executor::default();

        let keys: Vec<NodeKey> = ports
            .iter()
            .enumerate()
            .map(|(i, &(n_in, n_out))| {
                executor.graph.add_node(LegatoNode::new(
                    format!("n{i}"),
                    "Affine".into(),
                    Box::new(Affine::new(n_in, n_out, 1.0, 0.25)),
                ))
            })
            .collect();

        let any_oversized = ports
            .iter()
            .any(|&(n_in, n_out)| n_in > MAX_ARITY || n_out > MAX_ARITY);

        prop_assert_eq!(
            executor.validate_arity().is_err(),
            any_oversized,
            "validate_arity disagreed with the port counts {:?}",
            ports
        );

        // Whatever it accepts, the executor must actually be able to run.
        if !any_oversized {
            executor.set_sink(*keys.last().unwrap()).unwrap();
            executor.prepare(BLOCK);

            let view = executor.process(&mut context());
            prop_assert_eq!(view.chans, ports.last().unwrap().1);
        }
    }
}
