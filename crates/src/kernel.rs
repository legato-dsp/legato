//! The kernel resolution engine and per-sample executor.
//!
//! A `kernel` DSL declaration shares the `patch` body grammar, but instead of
//! being inlined into the block-rate graph it is lowered *whole* into one
//! [`KernelGraph`]: a flat, enum-dispatched per-sample subgraph. Because the
//! interior runs one sample at a time, feedback cycles are legal — any edge
//! that closes a cycle reads the value its source produced on the *previous*
//! sample (an implicit z⁻¹).
//!
//! This module is deliberately separate from the block-rate passes in
//! [`crate::dsl`]: those assume acyclic graphs and node multiplicity, neither
//! of which applies inside a kernel.

use crate::{
    builder::{ResourceBuilderView, ValidationError},
    dsl::{
        expand::substitute_templates,
        ir::{DSLParams, IRMacro, IRNodeKind, NodeId, NodeSelector, Object, Port},
    },
    msg::NodeMessage,
    nodes::audio::{
        allpass::Allpass,
        onepole::OnePole,
        ops::{ApplyOp, ApplyOpKind, mult_node_factory},
        saw::Saw,
        sine::Sine,
        svf::Svf,
        tap::DelayTap,
    },
    persample::{MAX_FRAME_PORTS, PerSampleNode},
    ports::{PortMeta, Ports},
};
use std::collections::HashMap;

/// A finite enum of per-sample nodes.
///
/// We're using this to prevent the v-table jump with
/// per-sample processing, as this can kill performance.
///
/// We still have custom nodes, if feedback is required
/// and your kernel is not available.
#[derive(Clone)]
pub enum KernelNode {
    Sine(Sine),
    Saw(Saw),
    Svf(Svf),
    OnePole(OnePole),
    Allpass(Allpass),
    Tap(DelayTap),
    Op(ApplyOp),
}

/// This macro lets us quickly write rules for all kernels
macro_rules! dispatch {
    ($self:expr, $inner:ident => $body:expr) => {
        match $self {
            KernelNode::Sine($inner) => $body,
            KernelNode::Saw($inner) => $body,
            KernelNode::Svf($inner) => $body,
            KernelNode::OnePole($inner) => $body,
            KernelNode::Allpass($inner) => $body,
            KernelNode::Tap($inner) => $body,
            KernelNode::Op($inner) => $body,
        }
    };
}

impl PerSampleNode for KernelNode {
    fn ports(&self) -> &Ports {
        dispatch!(self, inner => PerSampleNode::ports(inner))
    }

    fn tick(&mut self, in_frame: &[Option<f32>], out_frame: &mut [f32]) {
        dispatch!(self, inner => inner.tick(in_frame, out_frame))
    }

    fn handle_msg(&mut self, msg: NodeMessage) {
        dispatch!(self, inner => PerSampleNode::handle_msg(inner, msg))
    }
}

/// Build a node from kernels that process one sample at a time.
pub fn build_kernel_node(
    node_type: &str,
    rb: &mut ResourceBuilderView,
    p: &DSLParams,
) -> Result<KernelNode, ValidationError> {
    let op = |kind: ApplyOpKind, default: f32, chans: usize, p: &DSLParams| {
        mult_node_factory(
            p.get_f32("val").unwrap_or(default),
            p.get_usize("chans").unwrap_or(chans),
            kind,
        )
    };

    Ok(match node_type {
        "sine" => KernelNode::Sine(Sine::from_params(rb, p)?),
        "saw" => KernelNode::Saw(Saw::from_params(rb, p)?),
        "svf" => KernelNode::Svf(Svf::from_params(rb, p)?),
        "onepole" => KernelNode::OnePole(OnePole::from_params(rb, p)?),
        "allpass" => KernelNode::Allpass(Allpass::from_params(rb, p)?),
        "tap" => KernelNode::Tap(DelayTap::from_params(rb, p)?),
        // These match block rate defaults, perhaps we make a single source of truth in the future?
        "mult" => KernelNode::Op(op(ApplyOpKind::Mult, 1.0, 1, p)),
        "add" => KernelNode::Op(op(ApplyOpKind::Add, 0.0, 1, p)),
        "sub" => KernelNode::Op(op(ApplyOpKind::Subtract, 0.0, 1, p)),
        "div" => KernelNode::Op(op(ApplyOpKind::Div, 0.0, 1, p)),
        "gain" => KernelNode::Op(op(ApplyOpKind::Gain, 1.0, 2, p)),
        other => {
            return Err(ValidationError::NotKernelCapable(format!(
                "node type '{other}' has no per-sample implementation"
            )));
        }
    })
}

/// Where one summed contribution to an interior input port comes from.
///
/// In english: we are modeling feedback using a one sample delay.
/// so the source for each node is either an external input, or
/// from the table we have constructed.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Src {
    /// Index into the kernel's exterior input frame (a virtual port).
    External(usize),
    /// Slot in the persistent output table.
    /// If it has the last sample, that is our one sample delay
    Internal(usize),
}

/// A per-sample subgraph, executable as one [`PerSampleNode`].
///
/// Nodes are stored in topological order of the cycle-broken interior graph.
/// Each node ticks straight into its slice of `values`, so downstream nodes
/// read current-sample values and feedback readers read previous-sample
/// values.
#[derive(Clone)]
pub struct KernelGraph {
    nodes: Vec<KernelNode>,
    /// wiring[i][port] = summed sources for node i's input port.
    wiring: Vec<Vec<Vec<Src>>>,
    /// One slot per interior output port. Persists across ticks.
    values: Box<[f32]>,
    /// First value slot of each node's outputs.
    value_offsets: Vec<usize>,
    out_counts: Vec<usize>,
    /// Value slots exposed as the kernel's exterior outputs (the sink's).
    out_slots: Vec<usize>,
    scratch_in: Box<[Option<f32>]>,
    ports: Ports,
}

impl PerSampleNode for KernelGraph {
    fn ports(&self) -> &Ports {
        &self.ports
    }

    fn tick(&mut self, in_frame: &[Option<f32>], out_frame: &mut [f32]) {
        for i in 0..self.nodes.len() {
            let node_wiring = &self.wiring[i];
            let n_in = node_wiring.len();

            for (port, srcs) in node_wiring.iter().enumerate() {
                let mut acc = 0.0;
                let mut patched = false;
                for src in srcs {
                    match *src {
                        Src::External(e) => {
                            if let Some(v) = in_frame[e] {
                                acc += v;
                                patched = true;
                            }
                        }
                        Src::Internal(slot) => {
                            acc += self.values[slot];
                            patched = true;
                        }
                    }
                }
                self.scratch_in[port] = patched.then_some(acc);
            }

            let off = self.value_offsets[i];
            let n_out = self.out_counts[i];
            self.nodes[i].tick(&self.scratch_in[..n_in], &mut self.values[off..off + n_out]);
        }

        for (out, &slot) in out_frame.iter_mut().zip(self.out_slots.iter()) {
            *out = self.values[slot];
        }
    }
}

// ---------------------------------------------------------------------------
// Resolution engine
// ---------------------------------------------------------------------------

fn unsupported(what: impl Into<String>) -> ValidationError {
    ValidationError::UnsupportedInKernel(what.into())
}

/// Resolve a [`Port`] against a port list to concrete indices.
///
/// `Port::None` selects every port; slices and strides are not supported
/// inside kernels (yet).
fn resolve_port(
    port: &Port,
    ports: &[PortMeta],
    node_alias: &str,
) -> Result<Vec<usize>, ValidationError> {
    match port {
        Port::None => Ok((0..ports.len()).collect()),
        Port::Index(i) => {
            if *i >= ports.len() {
                return Err(ValidationError::InvalidParameter(format!(
                    "kernel: port index {i} out of range for '{node_alias}' ({} ports)",
                    ports.len()
                )));
            }
            Ok(vec![*i])
        }
        Port::Named(name) => ports
            .iter()
            .find(|p| p.name == name)
            .map(|p| vec![p.index])
            .ok_or_else(|| {
                ValidationError::InvalidParameter(format!(
                    "kernel: no port named '{name}' on '{node_alias}'"
                ))
            }),
        Port::Slice(..) | Port::Stride { .. } => Err(unsupported(format!(
            "port slices/strides on '{node_alias}'"
        ))),
    }
}

/// Reverse DFS postorder over the interior adjacency: a valid topological
/// order of the graph with its cycle-closing (back) edges removed. Feedback
/// edges therefore read the previous sample's value at run time.
///
/// Which edge of a cycle "closes" it — and thus carries the implicit z⁻¹ —
/// follows declaration order: DFS roots are tried in the order nodes are
/// declared in the kernel body.
fn cycle_broken_order(n: usize, adj: &[Vec<usize>]) -> Vec<usize> {
    const WHITE: u8 = 0;
    const GRAY: u8 = 1;
    const BLACK: u8 = 2;

    let mut color = vec![WHITE; n];
    let mut postorder = Vec::with_capacity(n);

    for root in 0..n {
        if color[root] != WHITE {
            continue;
        }
        let mut stack: Vec<(usize, usize)> = vec![(root, 0)];
        color[root] = GRAY;

        while let Some(&mut (u, ref mut next_child)) = stack.last_mut() {
            if *next_child < adj[u].len() {
                let v = adj[u][*next_child];
                *next_child += 1;
                if color[v] == WHITE {
                    color[v] = GRAY;
                    stack.push((v, 0));
                }
                // GRAY = back edge (cycle: becomes the z⁻¹ read),
                // BLACK = forward/cross edge; neither affects the order here.
            } else {
                color[u] = BLACK;
                postorder.push(u);
                stack.pop();
            }
        }
    }

    postorder.reverse();
    postorder
}

/// Lower a kernel definition plus one instantiation's params into an
/// executable [`KernelGraph`].
pub fn lower_kernel(
    ir_macro: &IRMacro,
    instance_params: &Object,
    rb: &mut ResourceBuilderView,
) -> Result<KernelGraph, ValidationError> {
    // ── Params: defaults overlaid by the instantiation site ────────────────
    let mut resolved_params = ir_macro.default_params.clone().unwrap_or_default();
    for (k, v) in instance_params {
        resolved_params.insert(k.clone(), v.clone());
    }

    // ── Instantiate interior nodes (declaration order) ─────────────────────
    let mut nodes: Vec<KernelNode> = Vec::with_capacity(ir_macro.body.node_count());
    let mut id_to_idx: HashMap<NodeId, usize> = HashMap::new();
    let mut aliases: Vec<String> = Vec::with_capacity(ir_macro.body.node_count());

    for ir_node in ir_macro.body.nodes() {
        if ir_node.kind != IRNodeKind::Leaf {
            return Err(unsupported(format!(
                "nested patch/kernel '{}' inside kernel '{}'",
                ir_node.alias, ir_macro.name
            )));
        }
        if ir_node.count != 1 {
            return Err(unsupported(format!(
                "multi-spawn '{} * {}' inside kernel '{}'",
                ir_node.alias, ir_node.count, ir_macro.name
            )));
        }
        if !ir_node.pipes.is_empty() {
            return Err(unsupported(format!(
                "pipes on '{}' inside kernel '{}'",
                ir_node.alias, ir_macro.name
            )));
        }

        let mut params = ir_node.params.clone();
        substitute_templates(&mut params, &resolved_params);

        let node = build_kernel_node(&ir_node.node_type, rb, &DSLParams::new(&params))?;

        id_to_idx.insert(ir_node.id, nodes.len());
        aliases.push(ir_node.alias.clone());
        nodes.push(node);
    }

    // ── Port geometry ───────────────────────────────────────────────────────
    let in_counts: Vec<usize> = nodes.iter().map(|n| n.ports().audio_in.len()).collect();
    let out_counts: Vec<usize> = nodes.iter().map(|n| n.ports().audio_out.len()).collect();

    let mut value_offsets = Vec::with_capacity(nodes.len());
    let mut total_out = 0usize;
    for &c in &out_counts {
        value_offsets.push(total_out);
        total_out += c;
    }

    let mut wiring: Vec<Vec<Vec<Src>>> = in_counts.iter().map(|&c| vec![Vec::new(); c]).collect();

    // ── Interior edges ──────────────────────────────────────────────────────
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];

    for edge in ir_macro.body.edges() {
        if edge.source_selector != NodeSelector::Single
            || edge.sink_selector != NodeSelector::Single
        {
            return Err(unsupported("node selectors inside kernels".to_string()));
        }

        let src = id_to_idx[&edge.source];
        let snk = id_to_idx[&edge.sink];

        let src_ports = resolve_port(
            &edge.source_port,
            &nodes[src].ports().audio_out,
            &aliases[src],
        )?;
        let snk_ports = resolve_port(&edge.sink_port, &nodes[snk].ports().audio_in, &aliases[snk])?;

        // NumPy-style broadcast, mirroring the block-rate builder: zip when
        // equal, replicate one source, or sum many sources into one port.
        let pairs: Vec<(usize, usize)> = match (src_ports.len(), snk_ports.len()) {
            (a, b) if a == b => src_ports.into_iter().zip(snk_ports).collect(),
            (1, _) => snk_ports.into_iter().map(|t| (src_ports[0], t)).collect(),
            (_, 1) => src_ports.into_iter().map(|s| (s, snk_ports[0])).collect(),
            (a, b) => {
                return Err(ValidationError::InvalidParameter(format!(
                    "kernel '{}': cannot match port arity {a}:{b} between '{}' and '{}'",
                    ir_macro.name, aliases[src], aliases[snk]
                )));
            }
        };

        for (sp, tp) in pairs {
            wiring[snk][tp].push(Src::Internal(value_offsets[src] + sp));
        }
        adj[src].push(snk);
    }

    // ── Virtual (exterior) input ports ──────────────────────────────────────
    for (ext_idx, (_name, targets)) in ir_macro.virtual_input_map.iter().enumerate() {
        for (node_id, _selector, port) in targets {
            let idx = id_to_idx[node_id];
            let target_ports = resolve_port(port, &nodes[idx].ports().audio_in, &aliases[idx])?;
            for tp in target_ports {
                wiring[idx][tp].push(Src::External(ext_idx));
            }
        }
    }

    // ── Order: topological with cycles broken into z⁻¹ reads ───────────────
    let order = cycle_broken_order(nodes.len(), &adj);

    // Permute all per-node tables into execution order.
    let mut position = vec![0usize; nodes.len()];
    for (pos, &idx) in order.iter().enumerate() {
        position[idx] = pos;
    }

    let mut ordered_nodes: Vec<Option<KernelNode>> = nodes.into_iter().map(Some).collect();
    let nodes: Vec<KernelNode> = order
        .iter()
        .map(|&i| ordered_nodes[i].take().unwrap())
        .collect();
    let wiring: Vec<Vec<Vec<Src>>> = order
        .iter()
        .map(|&i| std::mem::take(&mut wiring[i]))
        .collect();
    let out_counts_ordered: Vec<usize> = order.iter().map(|&i| out_counts[i]).collect();
    // Note: `values` slots keep their *declaration order* layout via
    // value_offsets, so Src::Internal indices stay valid across the permute.
    let value_offsets_ordered: Vec<usize> = order.iter().map(|&i| value_offsets[i]).collect();

    // ── Exterior ports ──────────────────────────────────────────────────────
    let sink_idx = id_to_idx[&ir_macro.sink];

    let n_exterior_in = ir_macro.virtual_input_map.len();
    if n_exterior_in > MAX_FRAME_PORTS || out_counts[sink_idx] > MAX_FRAME_PORTS {
        return Err(unsupported(format!(
            "kernel '{}' exceeds {MAX_FRAME_PORTS} exterior ports",
            ir_macro.name
        )));
    }

    // Virtual port names come from the source as owned Strings, but PortMeta
    // wants &'static str. Leaking is fine: kernels are built once, at build
    // time, and live for the program.
    let audio_in: Vec<PortMeta> = ir_macro
        .virtual_input_map
        .keys()
        .enumerate()
        .map(|(i, name)| PortMeta {
            name: Box::leak(name.clone().into_boxed_str()),
            index: i,
        })
        .collect();

    let sink_out_ports = nodes[position[sink_idx]].ports().audio_out.clone();
    let out_slots: Vec<usize> = (0..out_counts[sink_idx])
        .map(|p| value_offsets[sink_idx] + p)
        .collect();

    let max_in = in_counts.iter().copied().max().unwrap_or(0);

    Ok(KernelGraph {
        nodes,
        wiring,
        values: vec![0.0; total_out].into_boxed_slice(),
        value_offsets: value_offsets_ordered,
        out_counts: out_counts_ordered,
        out_slots,
        scratch_in: vec![None; max_in].into_boxed_slice(),
        ports: Ports {
            audio_in,
            audio_out: sink_out_ports,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{BlockSize, Config},
        dsl::{lower::ast_to_graph, parse::legato_parser},
        resources::ResourceBuilder,
    };

    /// Parse `src`, lower it, and hand back the kernel definition `name`.
    fn kernel_def(src: &str, name: &str) -> IRMacro {
        let ast = legato_parser(src).expect("kernel test source should parse");
        let graph = ast_to_graph(ast);
        graph
            .macro_registry
            .get(name)
            .unwrap_or_else(|| panic!("kernel '{name}' missing from registry"))
            .clone()
    }

    fn build(def: &IRMacro, params: Object) -> Result<KernelGraph, ValidationError> {
        let config = Config::new(48_000, BlockSize::Block64, 1, 0);
        let mut resource_builder = ResourceBuilder::default();
        let mut external = HashMap::new();
        let mut delays = HashMap::new();
        let mut view = ResourceBuilderView {
            config: &config,
            resource_builder: &mut resource_builder,
            external_buffer_keys: &mut external,
            delay_keys: &mut delays,
        };
        lower_kernel(def, &params, &mut view)
    }

    /// y[n] = x[n] + fb * y[n-1], built from `add` + `mult` with a feedback
    /// edge. The mult -> add edge closes the cycle and must read the previous
    /// sample (implicit z⁻¹) — verified exactly against the recurrence.
    #[test]
    fn feedback_cycle_gets_unit_delay() {
        let src = r#"
            kernel fb_loop(fb = 0.5) {
                in audio_in

                audio {
                    add { val: 0.0 },
                    mult { val: $fb }
                }

                audio_in >> add[0]
                add >> mult[0]
                mult >> add[1]

                { add }
            }
            audio { sine }
            { sine }
        "#;

        let def = kernel_def(src, "fb_loop");
        let mut kg = build(&def, Object::new()).expect("kernel should build");

        assert_eq!(PerSampleNode::ports(&kg).audio_in.len(), 1);
        assert_eq!(PerSampleNode::ports(&kg).audio_out.len(), 1);

        let input = [1.0f32, 0.0, 0.0, 0.0, 2.0, 0.0];
        let mut expected_state = 0.0f32;
        let mut out = [0.0f32];

        for (n, &x) in input.iter().enumerate() {
            kg.tick(&[Some(x)], &mut out);
            expected_state = x + 0.5 * expected_state;
            assert_eq!(
                out[0], expected_state,
                "feedback recurrence diverged at sample {n}"
            );
        }
    }

    /// Declaration order must not dictate execution order: `mult` is declared
    /// before the `add` that feeds it, yet a forward chain has to flow within
    /// a single sample: out = (x + 1) * 2.
    #[test]
    fn topo_order_ignores_declaration_order() {
        let src = r#"
            kernel chain() {
                in audio_in

                audio {
                    mult { val: 2.0 },
                    add { val: 1.0 }
                }

                audio_in >> add[0]
                add >> mult[0]

                { mult }
            }
            audio { sine }
            { sine }
        "#;

        let def = kernel_def(src, "chain");
        let mut kg = build(&def, Object::new()).expect("kernel should build");

        let mut out = [0.0f32];
        kg.tick(&[Some(3.0)], &mut out);
        assert_eq!(out[0], 8.0, "chain must run add before mult in one sample");
    }

    /// Kernel default params flow into interior nodes via templates and are
    /// overridable at the instantiation site.
    #[test]
    fn instance_params_override_defaults() {
        let src = r#"
            kernel scaled(amount = 2.0) {
                in audio_in

                audio {
                    mult { val: $amount }
                }

                audio_in >> mult[0]

                { mult }
            }
            audio { sine }
            { sine }
        "#;

        let def = kernel_def(src, "scaled");

        let mut with_default = build(&def, Object::new()).unwrap();
        let mut out = [0.0f32];
        with_default.tick(&[Some(3.0)], &mut out);
        assert_eq!(out[0], 6.0);

        let mut overridden = build(&def, crate::object! { "amount" => 5.0f32 }).unwrap();
        overridden.tick(&[Some(3.0)], &mut out);
        assert_eq!(out[0], 15.0);
    }

    #[test]
    fn non_kernel_capable_node_errors() {
        let src = r#"
            kernel bad() {
                in audio_in

                audio {
                    sampler { sampler_name: "amen" }
                }

                audio_in >> sampler

                { sampler }
            }
            audio { sine }
            { sine }
        "#;

        let def = kernel_def(src, "bad");
        match build(&def, Object::new()) {
            Err(ValidationError::NotKernelCapable(_)) => {}
            Err(other) => panic!("expected NotKernelCapable, got {other:?}"),
            Ok(_) => panic!("expected NotKernelCapable, got a built kernel"),
        }
    }
}
