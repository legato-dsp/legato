use crate::dsl::{ir::*, pipeline::GraphPass, resolve::port_for_instance};
use indexmap::IndexMap;
use std::collections::HashMap;

/// Choose the source selector for the `i`-th instance of a macro, given the
/// source and macro node counts.
///
/// This is the selector-space analogue of [`crate::dsl::resolve::broadcast`]:
/// macro expansion runs *before* [`crate::dsl::spawn`], so the source multi-node
/// has not been split into concrete instances yet and the arity must be encoded
/// as a [`NodeSelector`] for spawn to materialise later.
///
/// - `n:n` (n > 1) → zip: instance `i` of the source pairs with instance `i`
/// - `n:1` / `1:n` / `1:1` → preserve the original selector (fan-in / broadcast)
/// - `n:m` (both > 1, n != m) → arity error
fn instance_source_selector(
    src_count: u32,
    macro_count: u32,
    i: usize,
    original: &NodeSelector,
) -> NodeSelector {
    match (src_count, macro_count) {
        (n, m) if n == m && n > 1 => NodeSelector::Index(i),
        (n, m) if n > 1 && m > 1 => {
            panic!("Cannot match node selection arity {n}:{m} during macro expansion")
        }
        _ => original.clone(),
    }
}

/// This pass expands all [`IRMacros`] into the interior nodes,
/// wires the new interior connections, then handles connections
/// in and out to the macro instance.
#[derive(Default)]
pub struct MacroExpansionPass;

const MAXIMUM_DEPTH: u8 = 16;

impl GraphPass for MacroExpansionPass {
    fn name(&self) -> &'static str {
        "MacroExpansionPass"
    }
    /// Expand macros while they still exist.
    fn run(&self, mut graph: IRGraph) -> IRGraph {
        let mut depth = 0u8;
        while graph.has_unresolved_macros() {
            assert!(
                depth < MAXIMUM_DEPTH,
                "MacroExpansionPass exceeded maximum depth — possible cycle in macro definitions"
            );
            let macro_ids: Vec<NodeId> = graph.macro_nodes().map(|n| n.id).collect();
            for id in macro_ids {
                self.expand_macro(&mut graph, id);
            }
            depth += 1;
        }
        graph
    }
}

impl MacroExpansionPass {
    fn expand_macro(&self, graph: &mut IRGraph, node_id: NodeId) {
        let node = graph.get_node(node_id).unwrap().clone();

        let ir_macro = graph
            .macro_registry
            .get(&node.node_type)
            .cloned()
            .unwrap_or_else(|| panic!("Macro '{}' not found in registry", node.node_type));

        let mut resolved_params = ir_macro.default_params.clone().unwrap_or_default();
        for (k, v) in &node.params {
            resolved_params.insert(k.clone(), v.clone());
        }

        let incoming: Vec<IREdge> = graph.incoming_edges(node_id).cloned().collect();
        let outgoing: Vec<IREdge> = graph.outgoing_edges(node_id).cloned().collect();
        graph.remove_node(node_id);

        // Expand n=count instances, each with a distinct alias prefix.
        let mut new_sinks: Vec<NodeId> = Vec::with_capacity(node.count as usize);

        for i in 0..node.count as usize {
            let instance_alias = if node.count == 1 {
                node.alias.clone()
            } else {
                format!("{}.{}", node.alias, i)
            };

            let id_map = self.clone_body_into(graph, &ir_macro, &instance_alias, &resolved_params);
            let new_sink = id_map[&ir_macro.sink];
            new_sinks.push(new_sink);

            let remapped_virtual: IndexMap<String, Vec<(NodeId, NodeSelector, Port)>> = ir_macro
                .virtual_input_map
                .iter()
                .map(|(name, targets)| {
                    let remapped = targets
                        .iter()
                        .map(|(id, sel, port)| (id_map[id], sel.clone(), port.clone()))
                        .collect();
                    (name.clone(), remapped)
                })
                .collect();

            // Rewire incoming edges into each instance.
            for edge in &incoming {
                // A strided/sliced source port is distributed one index per macro
                // instance — but only when there are several instances. A single
                // instance keeps the port intact (e.g. its full stereo slice) so
                // the builder can fan it across channels rather than collapsing it.
                let resolved_source_port = if node.count > 1 {
                    port_for_instance(&edge.source_port, i)
                } else {
                    edge.source_port.clone()
                };

                // Encode the source/macro arity as a selector for spawn to expand.
                let src_count = graph.get_node(edge.source).map(|n| n.count).unwrap_or(1);
                let resolved_source_selector =
                    instance_source_selector(src_count, node.count, i, &edge.source_selector);

                let targets: Vec<(NodeId, NodeSelector, Port)> = match &edge.sink_port {
                    Port::Named(name) => remapped_virtual.get(name).cloned().unwrap_or_else(|| {
                        vec![(new_sink, NodeSelector::Single, edge.sink_port.clone())]
                    }),
                    Port::Index(i) => remapped_virtual
                        .get_index(*i)
                        .map(|(_, v)| v.clone())
                        .unwrap_or_else(|| {
                            vec![(new_sink, NodeSelector::Single, edge.sink_port.clone())]
                        }),
                    Port::None => remapped_virtual
                        .get_index(0)
                        .map(|(_, v)| v.clone())
                        .unwrap_or_else(|| vec![(new_sink, NodeSelector::Single, Port::None)]),
                    Port::Slice(..) | Port::Stride { .. } => panic!(
                        "Slice/Stride not supported on virtual ports (macro '{}')",
                        node.node_type
                    ),
                };
                for (target_id, target_selector, target_port) in targets {
                    graph.connect_multi(
                        edge.source,
                        resolved_source_selector.clone(),
                        resolved_source_port.clone(),
                        target_id,
                        target_selector,
                        target_port,
                    );
                }
            }
        }

        // Rewire outgoing edges from the last instance
        for edge in &outgoing {
            let srcs = edge.source_selector.select(&new_sinks).to_vec();
            let multi_src = srcs.len() > 1;
            for (i, &src) in srcs.iter().enumerate() {
                // Distribute a strided/sliced sink port one index per source
                // instance only when several instances share it. A single
                // instance preserves the slice so the builder fans it across
                // the sink's channels (e.g. a stereo macro into `mixer[0..2]`).
                let resolved_sink_port = if multi_src {
                    port_for_instance(&edge.sink_port, i)
                } else {
                    edge.sink_port.clone()
                };
                graph.connect_multi(
                    src,
                    NodeSelector::Single,
                    edge.source_port.clone(),
                    edge.sink,
                    edge.sink_selector.clone(),
                    resolved_sink_port,
                );
            }
        }

        if graph.sink == Some(node_id) {
            graph.sink = new_sinks.last().copied();
        }
        if graph.source == Some(node_id) {
            graph.source = new_sinks.first().copied();
        }
    }

    /// Clone an IRMacro's body into [`IRGraph`], prefixing all aliases.
    fn clone_body_into(
        &self,
        graph: &mut IRGraph,
        ir_macro: &IRMacro,
        instance_alias: &str,
        resolved_params: &Object,
    ) -> HashMap<NodeId, NodeId> {
        let mut id_map: HashMap<NodeId, NodeId> = HashMap::new();

        // A this point, everything should be a leaf!
        for node in ir_macro.body.nodes() {
            let fqn = format!("{}.{}", instance_alias, node.alias);

            let mut params = node.params.clone();
            substitute_templates(&mut params, resolved_params);

            let new_id = graph.add_node(
                node.kind.clone(),
                node.namespace.clone(),
                node.node_type.clone(),
                fqn,
                params,
                node.count,
            );
            id_map.insert(node.id, new_id);
        }

        // Clone edges
        for edge in ir_macro.body.edges() {
            graph.reconnect(id_map[&edge.source], id_map[&edge.sink], edge);
        }

        id_map
    }
}

/// Replace `$name` template values in `params` with their bindings from
/// `lookup`. Shared between patch expansion and kernel lowering.
pub fn substitute_templates(params: &mut Object, lookup: &Object) {
    for val in params.values_mut() {
        if let Value::Template(tpl) = val {
            let key = tpl.trim_start_matches('$');
            if let Some(replacement) = lookup.get(key) {
                *val = replacement.clone();
            }
        }
    }
}
