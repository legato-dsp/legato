use crate::builder::ValidationError;
use crate::dsl::{
    ir::*,
    pipeline::GraphPass,
    resolve::{Plan, broadcast, port_for_instance, source_ports},
};
use std::collections::HashMap;

#[derive(Default)]
pub struct SpawnKNodesPass;

impl GraphPass for SpawnKNodesPass {
    fn name(&self) -> &'static str {
        "SpawnKNodesPass"
    }

    fn run(&self, graph: IRGraph) -> Result<IRGraph, ValidationError> {
        self.expand_nodes(graph)
    }
}

impl SpawnKNodesPass {
    fn expand_nodes(&self, mut graph: IRGraph) -> Result<IRGraph, ValidationError> {
        let multi: Vec<(NodeId, IRNode)> = graph
            .nodes()
            .filter(|n| n.count > 1)
            .map(|n| (n.id, n.clone()))
            .collect();

        if multi.is_empty() {
            return Ok(graph);
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

            Self::expand_edge(&mut graph, edge, &srcs, &snks)?;
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

        Ok(graph)
    }

    /// Wire up a concrete set of source and sink NodeIds according to the
    /// original edge's port configuration.
    fn expand_edge(
        graph: &mut IRGraph,
        edge: &IREdge,
        srcs: &[NodeId],
        snks: &[NodeId],
    ) -> Result<(), ValidationError> {
        // Coupled case: N source instances zipped onto a contiguous port slice
        // of a single sink (e.g. `src(0..N) >> mixer[a..b]`). The instance index
        // selects the concrete port, so this is its own zip rather than a plain
        // node-level broadcast.
        if let Port::Slice(start, end) = &edge.sink_port {
            let width = end - start;
            let ports = source_ports(&edge.source_port);
            // Exact zip: instances x source-ports must equal the slice width, so
            // there is a single flatten axis and no cross-product inference.
            if srcs.len() * ports.len() != width {
                return Err(ValidationError::SelectionArity(format!(
                    "source lines ({} instances x {} ports) must equal port slice width ({width})",
                    srcs.len(),
                    ports.len(),
                )));
            }
            // Flatten instance-major: instance i owns ports [i*len, i*len + len).
            for (i, &src) in srcs.iter().enumerate() {
                for (offset, source_port) in ports.iter().enumerate() {
                    graph.connect(
                        src,
                        source_port.clone(),
                        snks[0],
                        Port::Index(start + i * ports.len() + offset),
                    );
                }
            }
            return Ok(());
        }

        // Dual of the coupled case above: a *single* source addressed by a
        // strided/sliced source port, fanned across N sink instances — e.g.
        // `poly_voice[1:15:3] >> voice(*).freq`. The instance index selects the
        // concrete source port, so `poly_voice[1 + 3i] -> voice#i.freq`. Without
        // this, node-level broadcast (1:N) hands the *whole* stride to every
        // instance, and the builder then sums it (fan-in) into each sink port —
        // e.g. every voice would receive all five voices' frequencies summed.
        if srcs.len() == 1
            && snks.len() > 1
            && matches!(edge.source_port, Port::Stride { .. } | Port::Slice(..))
        {
            for (i, &snk) in snks.iter().enumerate() {
                graph.connect(
                    srcs[0],
                    port_for_instance(&edge.source_port, i),
                    snk,
                    edge.sink_port.clone(),
                );
            }
            return Ok(());
        }

        // All other node-level multiplicity follows the shared broadcasting rule.
        let connect = |graph: &mut IRGraph, src: NodeId, snk: NodeId| {
            graph.connect(src, edge.source_port.clone(), snk, edge.sink_port.clone());
        };
        match broadcast(srcs, snks).map_err(|e| ValidationError::SelectionArity(e.to_string()))? {
            Plan::Zip(pairs) => {
                for (src, snk) in pairs {
                    connect(graph, src, snk);
                }
            }
            Plan::OneToMany(src, snks) => {
                for snk in snks {
                    connect(graph, src, snk);
                }
            }
            Plan::ManyToOne(srcs, snk) => {
                for src in srcs {
                    connect(graph, src, snk);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::dsl::{ir::Port, parse::legato_parser, pipeline::Pipeline};

    /// A strided *source* port fanned across `* N` leaf/kernel instances must
    /// distribute one port per instance (`src[start + i·stride] -> inst#i`), not
    /// broadcast the whole stride into every instance (which the builder then
    /// sums as a fan-in). This was the poly.rs "click + echoes" bug: patches
    /// distribute via `expand.rs`, but leaf/kernel `* N` spawns come through
    /// this pass, which previously lacked the coupled-distribution rule.
    #[test]
    fn strided_source_distributes_across_spawned_instances() {
        // poly_voice lays out [gate, freq, vel] per voice; `[1:6:3]` selects the
        // two freq ports (1, 4). Two `onepole` instances stand in for a `* 2`
        // kernel spawn — both go through this pass as leaf nodes.
        let src = r#"
            audio {
                onepole: v * 2 { cutoff: 500.0 }
            }
            midi {
                poly_voice { chan: 0, voices: 2 }
            }
            poly_voice[1:6:3] >> v(*)[0]
            { v }
        "#;
        let ast = legato_parser(src).expect("test source should parse");
        let graph = Pipeline::default()
            .run_from_ast(ast)
            .expect("test source should lower");

        let source_ports = |snk: &str| -> Vec<Port> {
            graph
                .find_edges_between("poly_voice", snk)
                .into_iter()
                .map(|e| e.source_port.clone())
                .collect()
        };

        // v.0 reads poly_voice[1], v.1 reads poly_voice[4] (1 + 1·3) — one edge
        // each, not the whole [1, 4] stride summed into both.
        assert_eq!(source_ports("v.0"), vec![Port::Index(1)]);
        assert_eq!(source_ports("v.1"), vec![Port::Index(4)]);
    }
}
