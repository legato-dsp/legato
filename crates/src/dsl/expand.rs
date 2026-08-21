use crate::builder::ValidationError;
use crate::dsl::{
    ir::*,
    pipeline::GraphPass,
    resolve::{port_for_instance, source_ports},
};
use indexmap::IndexMap;
use std::collections::HashMap;

/// Pair the far side of a connection against the near side, by *selected*
/// instance; `None` when the two selections cannot be matched.
///
/// Expansion runs before [`crate::dsl::spawn`], so the far side may still be an
/// unexpanded multi-node and the pairing has to be encoded as a
/// [`NodeSelector`] for spawn to materialise later.
///
/// - `n:n` (n > 1) → zip, pairing by position within each selection
/// - `n:1` / `1:n` / `1:1` → preserve the original selector (fan-in / broadcast)
/// - `n:m` (both > 1, n != m) → `None`
fn pair_selector(
    far: &[usize],
    pos: usize,
    near: usize,
    original: &NodeSelector,
) -> Option<NodeSelector> {
    match (far.len(), near) {
        (n, m) if n == m && n > 1 => Some(NodeSelector::Index(far[pos])),
        (n, m) if n > 1 && m > 1 => None,
        _ => Some(original.clone()),
    }
}

/// The instance indices `sel` picks from the node it addresses.
fn selection_of(graph: &IRGraph, node: NodeId, sel: &NodeSelector) -> Vec<usize> {
    let count = graph.get_node(node).map(|n| n.count).unwrap_or(1);
    sel.selected_indices(count as usize)
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
    fn run(&self, mut graph: IRGraph) -> Result<IRGraph, ValidationError> {
        let mut depth = 0u8;
        while graph.has_unresolved_macros() {
            if depth >= MAXIMUM_DEPTH {
                return Err(ValidationError::Expansion(format!(
                    "macro expansion exceeded depth {MAXIMUM_DEPTH} — patches may be mutually recursive"
                )));
            }
            let macro_ids: Vec<NodeId> = graph.macro_nodes().map(|n| n.id).collect();
            for id in macro_ids {
                self.expand_macro(&mut graph, id)?;
            }
            depth += 1;
        }
        Ok(graph)
    }
}

impl MacroExpansionPass {
    fn expand_macro(&self, graph: &mut IRGraph, node_id: NodeId) -> Result<(), ValidationError> {
        let node = graph
            .get_node(node_id)
            .expect("macro node vanished between listing and expanding")
            .clone();

        let ir_macro = graph
            .macro_registry
            .get(&node.node_type)
            .cloned()
            .ok_or_else(|| {
                ValidationError::NodeNotFound(format!(
                    "macro '{}' is not in the registry",
                    node.node_type
                ))
            })?;

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
                // A sink-side selector picks instances, exactly as it does for a
                // multi-node leaf in `spawn`: `src >> voice(0)` wires one voice.
                let picked = edge.sink_selector.selected_indices(node.count as usize);
                let Some(pos) = picked.iter().position(|&x| x == i) else {
                    continue;
                };

                // A strided/sliced source port is distributed one index per macro
                // instance — but only when there are several instances. A single
                // instance keeps the port intact (e.g. its full stereo slice) so
                // the builder can fan it across channels rather than collapsing it.
                let resolved_source_port = if node.count > 1 {
                    port_for_instance(&edge.source_port, i)
                } else {
                    edge.source_port.clone()
                };

                // Pair the source selection against this instance's position.
                let sources = selection_of(graph, edge.source, &edge.source_selector);
                let resolved_source_selector =
                    pair_selector(&sources, pos, picked.len(), &edge.source_selector).ok_or_else(
                        || {
                            ValidationError::SelectionArity(format!(
                                "cannot match selection arity {}:{} into patch '{}'",
                                sources.len(),
                                picked.len(),
                                node.node_type
                            ))
                        },
                    )?;

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
                    Port::Slice(..) | Port::Stride { .. } => {
                        return Err(ValidationError::Expansion(format!(
                            "slice and stride ports are not supported on the virtual ports of \
                             patch '{}'",
                            node.node_type
                        )));
                    }
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
            // Splitting one edge per source instance would otherwise let each
            // half broadcast independently, giving a cross product.
            let sinks = selection_of(graph, edge.sink, &edge.sink_selector);

            // Coupled flatten: N instances zipped instance-major onto a single
            // sink's port slice. Instances x source-ports must equal the slice
            // width exactly, so there is one flatten axis and no cross-product.
            if let (true, 1, Port::Slice(start, end)) = (multi_src, sinks.len(), &edge.sink_port) {
                let width = end - start;
                let ports = source_ports(&edge.source_port);
                if srcs.len() * ports.len() != width {
                    return Err(ValidationError::SelectionArity(format!(
                        "source lines ({} instances x {} ports) must equal sink port slice width ({width}) out of patch '{}'",
                        srcs.len(),
                        ports.len(),
                        node.node_type,
                    )));
                }
                for (i, &src) in srcs.iter().enumerate() {
                    for (offset, source_port) in ports.iter().enumerate() {
                        graph.connect_multi(
                            src,
                            NodeSelector::Single,
                            source_port.clone(),
                            edge.sink,
                            NodeSelector::Single,
                            Port::Index(start + i * ports.len() + offset),
                        );
                    }
                }
                continue;
            }

            for (i, &src) in srcs.iter().enumerate() {
                let resolved_sink_selector =
                    pair_selector(&sinks, i, srcs.len(), &edge.sink_selector).ok_or_else(|| {
                        ValidationError::SelectionArity(format!(
                            "cannot match selection arity {}:{} out of patch '{}'",
                            srcs.len(),
                            sinks.len(),
                            node.node_type
                        ))
                    })?;

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
                    resolved_sink_selector,
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
        Ok(())
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
