use crate::dsl::{ir::*, pipeline::GraphPass};
use std::collections::HashMap;

#[derive(Default)]
pub struct SpawnKNodesPass;

impl GraphPass for SpawnKNodesPass {
    fn name(&self) -> &'static str {
        "SpawnKNodesPass"
    }

    fn run(&self, graph: IRGraph) -> IRGraph {
        self.expand_nodes(graph)
    }
}

impl SpawnKNodesPass {
    fn expand_nodes(&self, mut graph: IRGraph) -> IRGraph {
        let multi: Vec<(NodeId, IRNode)> = graph
            .nodes()
            .filter(|n| n.count > 1)
            .map(|n| (n.id, n.clone()))
            .collect();

        if multi.is_empty() {
            return graph;
        }

        let multi_ids: std::collections::HashSet<NodeId> =
            multi.iter().map(|(id, _)| *id).collect();

        // ── Phase 1: spawn N instances for every multi-node ────────────────
        let mut expansion: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

        for (orig_id, node) in &multi {
            let mut instances = Vec::with_capacity(node.count as usize);
            for i in 0..node.count as usize {
                let alias = format!("{}.{}", node.alias, i);
                let new_id = graph.add_node(
                    node.kind.clone(),
                    node.namespace.clone(),
                    node.node_type.clone(),
                    alias,
                    node.params.clone(),
                    node.pipes.clone(),
                    1,
                );
                instances.push(new_id);
            }
            expansion.insert(*orig_id, instances);
        }

        // ── Phase 2: expand edges that touch any multi-node ────────────────
        let snapshot: Vec<IREdge> = graph.edges().to_vec();

        for edge in &snapshot {
            let src_multi = multi_ids.contains(&edge.source);
            let snk_multi = multi_ids.contains(&edge.sink);
            if !src_multi && !snk_multi {
                continue; // unaffected — left in place
            }

            let src_pool: Vec<NodeId> = if src_multi {
                expansion[&edge.source].clone()
            } else {
                vec![edge.source]
            };
            let snk_pool: Vec<NodeId> = if snk_multi {
                expansion[&edge.sink].clone()
            } else {
                vec![edge.sink]
            };

            let srcs = edge.source_selector.select(&src_pool).to_vec();
            let snks = edge.sink_selector.select(&snk_pool).to_vec();

            Self::expand_edge(&mut graph, edge, &srcs, &snks);
        }

        // ── Phase 3: remove originals (also removes their incident edges) ──
        for (orig_id, _) in &multi {
            if graph.sink == Some(*orig_id) {
                // Last instance is the natural graph output.
                graph.sink = expansion[orig_id].last().copied();
            }
            if graph.source == Some(*orig_id) {
                graph.source = expansion[orig_id].first().copied();
            }
            graph.remove_node(*orig_id);
        }

        graph
    }

    /// Wire up a concrete set of source and sink NodeIds according to the
    /// original edge's port configuration.
    fn expand_edge(graph: &mut IRGraph, edge: &IREdge, srcs: &[NodeId], snks: &[NodeId]) {
        match &edge.sink_port {
            Port::Slice(start, end) => {
                assert_eq!(
                    srcs.len(),
                    end - start,
                    "SpawnKNodesPass: source instance count ({}) must equal \
                     port slice width ({}) for edge to {:?}",
                    srcs.len(),
                    end - start,
                    edge.sink,
                );
                let snk = snks[0];
                for (i, &src) in srcs.iter().enumerate() {
                    graph.connect(src, edge.source_port.clone(), snk, Port::Index(start + i));
                }
            }
            _ => match (srcs.len(), snks.len()) {
                (1, _) => {
                    // Broadcast: one source -> all sinks.
                    for &snk in snks {
                        graph.connect(
                            srcs[0],
                            edge.source_port.clone(),
                            snk,
                            edge.sink_port.clone(),
                        );
                    }
                }
                (_, 1) => {
                    // Fan-in: all sources -> single sink.
                    for &src in srcs {
                        graph.connect(
                            src,
                            edge.source_port.clone(),
                            snks[0],
                            edge.sink_port.clone(),
                        );
                    }
                }
                (n, m) => {
                    // Automap / zip.
                    assert_eq!(
                        n, m,
                        "SpawnKNodesPass: cannot automap nodes with different \
                         instance counts ({n} vs {m})"
                    );
                    for (&src, &snk) in srcs.iter().zip(snks.iter()) {
                        graph.connect(src, edge.source_port.clone(), snk, edge.sink_port.clone());
                    }
                }
            },
        }
    }
}
