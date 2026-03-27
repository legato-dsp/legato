use crate::dsl::{ir::*, pipeline::GraphPass};
use indexmap::IndexMap;
use std::collections::HashMap;

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

            let remapped_virtual: IndexMap<String, (NodeId, NodeSelector, Port)> = ir_macro
                .virtual_input_map
                .iter()
                .map(|(name, (id, sel, port))| {
                    (name.clone(), (id_map[id], sel.clone(), port.clone()))
                })
                .collect();

            // Rewire incoming edges into each instance.
            for edge in &incoming {
                let (target_id, target_selector, target_port) = match &edge.sink_port {
                    Port::Named(name) => remapped_virtual.get(name).map_or(
                        (new_sink, NodeSelector::Single, edge.sink_port.clone()),
                        |(id, sel, port)| (*id, sel.clone(), port.clone()),
                    ),
                    Port::Index(i) => remapped_virtual.get_index(*i).map_or(
                        (new_sink, NodeSelector::Single, edge.sink_port.clone()),
                        |(_, (id, sel, port))| (*id, sel.clone(), port.clone()),
                    ),
                    Port::None => remapped_virtual.get_index(0).map_or(
                        (new_sink, NodeSelector::Single, Port::None),
                        |(_, (id, sel, port))| (*id, sel.clone(), port.clone()),
                    ),
                    Port::Slice(..) | Port::Stride { .. } => panic!(
                        "Slice/Stride not supported on virtual ports (macro '{}')",
                        node.node_type
                    ),
                };
                graph.connect_multi(
                    edge.source,
                    edge.source_selector.clone(),
                    edge.source_port.clone(),
                    target_id,
                    target_selector,
                    target_port,
                );
            }
        }

        // Rewire outgoing edges from the last instance
        for edge in &outgoing {
            let srcs = edge.source_selector.select(&new_sinks).to_vec();
            for &src in &srcs {
                graph.connect_multi(
                    src,
                    NodeSelector::Single,
                    edge.source_port.clone(),
                    edge.sink,
                    edge.sink_selector.clone(),
                    edge.sink_port.clone(),
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
            Self::substitute_templates(&mut params, resolved_params);

            let new_id = graph.add_node(
                node.kind.clone(),
                node.namespace.clone(),
                node.node_type.clone(),
                fqn,
                params,
                node.pipes.clone(),
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

    fn substitute_templates(params: &mut Object, lookup: &Object) {
        for val in params.values_mut() {
            if let Value::Template(tpl) = val {
                let key = tpl.trim_start_matches('$');
                if let Some(replacement) = lookup.get(key) {
                    *val = replacement.clone();
                }
            }
        }
    }
}
