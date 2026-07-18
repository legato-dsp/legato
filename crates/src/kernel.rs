use crate::{
    builder::{ResourceBuilderView, ValidationError},
    dsl::{
        expand::substitute_templates,
        ir::{DSLParams, IRMacro, IRNodeKind, NodeId, NodeSelector, Object, Port},
    },
    msg::NodeMessage,
    nodes::{
        audio::{
            allpass::Allpass,
            hadamard::HadamardMixer,
            householder::HouseholderMixer,
            noise::Noise,
            onepole::OnePole,
            ops::{ApplyOp, ApplyOpKind, mult_node_factory},
            pan::Pan,
            saw::Saw,
            sine::Sine,
            svf::Svf,
            tap::DelayTap,
        },
        control::map::Map,
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
    Map(Map),
    Noise(Noise),
    Householder(HouseholderMixer),
    Hadamard(HadamardMixer),
    Pan(Pan),
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
            KernelNode::Map($inner) => $body,
            KernelNode::Noise($inner) => $body,
            KernelNode::Householder($inner) => $body,
            KernelNode::Hadamard($inner) => $body,
            KernelNode::Pan($inner) => $body,
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
        "map" => KernelNode::Map(Map::from_params(rb, p)?),
        "noise" => KernelNode::Noise(Noise::new()),
        "householder" => KernelNode::Householder(HouseholderMixer::from_params(rb, p)?),
        "hadamard" => KernelNode::Hadamard(HadamardMixer::from_params(rb, p)?),
        "pan" => KernelNode::Pan(Pan::from_params(rb, p)?),
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

/// A node's position in kernel-body declaration order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct DeclIdx(usize);

/// A node's position in the final execution order
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ExecIdx(usize);

/// A slot in the persistent `values` table
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ValueSlot(u32);

/// An index into the kernel's exterior input frame
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ExternalInput(u32);

/// Where one summed contribution to an interior input port comes from.
///
/// In english: we are modeling feedback using a one sample delay.
/// so the source for each node is either an external input, or
/// from the table we have constructed.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Src {
    /// The kernel's exterior input frame (a virtual port).
    External(ExternalInput),
    /// A slot in the persistent output table.
    /// If it has the last sample, that is our one sample delay.
    Internal(ValueSlot),
}

/// Per-node indexing into the flat runtime tables, in execution order.
#[derive(Clone, Copy, Debug)]
struct NodeLayout {
    /// First entry in `port_sources` for this node
    first_in_port: u32,
    n_in: u32,
    /// First `values` slot of this node's outputs.
    first_value_slot: u32,
    n_out: u32,
}

/// A per-sample subgraph, executable as one [`PerSampleNode`].
///
/// Nodes are stored in topological order of the cycle-broken interior graph.
/// Each node ticks straight into its slice of `values`, so downstream nodes
/// read current-sample values and feedback readers read previous-sample
/// values.
///
/// All wiring is held in two flat, contiguous tables rather than nested Vecs
/// so the per-sample walk touches sequential memory:
/// - `port_sources[p] = (start, len)` — the sources for input port `p` are
///   `src_pool[start..start + len]`, with ports grouped per node in
///   execution order.
/// - `src_pool` — every [`Src`] in the kernel, in that same walk order.
#[derive(Clone)]
pub struct KernelGraph {
    /// Interior nodes in execution order.
    nodes: Vec<KernelNode>,
    /// Geometry per node, parallel to `nodes`.
    layouts: Box<[NodeLayout]>,
    /// `(start, len)` into `src_pool` for each input port.
    port_sources: Box<[(u32, u32)]>,
    /// The flattened source lists behind `port_sources`.
    src_pool: Box<[Src]>,
    /// One slot per interior output port. Persists across ticks — this
    /// persistence *is* the z⁻¹ on feedback edges (see `tick`).
    values: Box<[f32]>,
    /// Value slots exposed as the kernel's exterior outputs (the sink's).
    out_slots: Box<[ValueSlot]>,
    scratch_in: Box<[Option<f32>]>,
    ports: Ports,
}

impl PerSampleNode for KernelGraph {
    fn ports(&self) -> &Ports {
        &self.ports
    }

    fn tick(&mut self, in_frame: &[Option<f32>], out_frame: &mut [f32]) {
        for i in 0..self.nodes.len() {
            let layout = self.layouts[i];

            for p in 0..layout.n_in as usize {
                let (start, len) = self.port_sources[layout.first_in_port as usize + p];
                let sources = &self.src_pool[start as usize..(start + len) as usize];

                let mut acc = 0.0;
                let mut patched = false;
                for src in sources {
                    match *src {
                        Src::External(ExternalInput(e)) => {
                            if let Some(v) = in_frame[e as usize] {
                                acc += v;
                                patched = true;
                            }
                        }
                        // This read is where feedback gets its one-sample
                        // delay. This comes from the previous sample, assuming
                        // we had a cycle and broke it here.
                        Src::Internal(ValueSlot(slot)) => {
                            acc += self.values[slot as usize];
                            patched = true;
                        }
                    }
                }
                self.scratch_in[p] = patched.then_some(acc);
            }

            let first = layout.first_value_slot as usize;
            let n_out = layout.n_out as usize;

            self.nodes[i].tick(
                &self.scratch_in[..layout.n_in as usize],
                &mut self.values[first..first + n_out],
            );
        }

        for (out, &ValueSlot(slot)) in out_frame.iter_mut().zip(self.out_slots.iter()) {
            *out = self.values[slot as usize];
        }
    }
}

fn unsupported(what: impl Into<String>) -> ValidationError {
    ValidationError::UnsupportedInKernel(what.into())
}

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

/// DFS visit state, i.e. the classic three "colors" of an edge-classifying
/// depth-first search (white / gray / black in CLRS §22.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VisitState {
    /// Not reached yet ("white").
    Unvisited,
    /// Currently on the DFS path — an ancestor of the node being explored
    /// ("gray"). An edge pointing *into* an `OnPath` node closes a cycle:
    /// that is a **back edge**.
    OnPath,
    /// Fully explored, all descendants finished ("black").
    Finished,
}

/// Compute an execution order for a possibly-cyclic kernel body.
///
/// **Algorithm: DFS edge classification + reverse postorder** — the standard
/// DFS-based topological sort (CLRS §22.4), generalized to cyclic graphs by
/// simply *not descending* through back edges. During the search, every edge
/// `from -> to` is classified by `to`'s [`VisitState`]:
///
/// - `Unvisited` → *tree edge*: descend into `to`.
/// - `OnPath`    → *back edge*: `to` is an ancestor still on the DFS path,
///   so this edge closes a cycle. We skip it, which is equivalent to
///   deleting it from the graph (the set of skipped back edges is a
///   feedback arc set — removing them leaves a DAG).
/// - `Finished`  → *forward/cross edge*: already handled, skip.
///
/// Listing nodes in **reverse finishing order** then yields a topological
/// order of that DAG: for every non-back edge `from -> to`, `to` finishes
/// first and therefore sorts *after* `from`.
///
/// Runtime meaning of a back edge: its reader executes before its source
/// each tick, so the reader sees the source's value-table slot from the
/// *previous* tick — the implicit z⁻¹ that makes feedback loops legal.
/// DFS roots are tried in declaration order, so *which* edge of a cycle
/// becomes the delayed one follows the order nodes appear in the kernel
/// body.
fn execution_order(node_count: usize, successors: &[Vec<DeclIdx>]) -> Vec<DeclIdx> {
    let mut state = vec![VisitState::Unvisited; node_count];
    let mut finish_order: Vec<DeclIdx> = Vec::with_capacity(node_count);

    for root in 0..node_count {
        if state[root] != VisitState::Unvisited {
            continue;
        }

        // Iterative DFS. Each frame is (node, index of the next successor to
        // classify); a frame is finished once every successor is classified.
        let mut path: Vec<(DeclIdx, usize)> = vec![(DeclIdx(root), 0)];
        state[root] = VisitState::OnPath;

        while let Some(&mut (node, ref mut next_successor)) = path.last_mut() {
            if let Some(&target) = successors[node.0].get(*next_successor) {
                *next_successor += 1;
                if state[target.0] == VisitState::Unvisited {
                    // Tree edge: descend.
                    state[target.0] = VisitState::OnPath;
                    path.push((target, 0));
                }
                // OnPath: back edge — the cycle breaks here (z⁻¹ read).
                // Finished: forward/cross edge — nothing to do.
            } else {
                // All successors classified: this node is finished.
                state[node.0] = VisitState::Finished;
                finish_order.push(node);
                path.pop();
            }
        }
    }

    finish_order.reverse();
    finish_order
}

/// Lower a kernel definition plus one instantiation's params into an
/// executable [`KernelGraph`].
pub fn lower_kernel(
    ir_macro: &IRMacro,
    instance_params: &Object,
    rb: &mut ResourceBuilderView,
) -> Result<KernelGraph, ValidationError> {
    // Resolve default parameters
    let mut resolved_params = ir_macro.default_params.clone().unwrap_or_default();
    for (k, v) in instance_params {
        resolved_params.insert(k.clone(), v.clone());
    }

    // Spawn interior nodes in declaration order. Everything below that is
    // indexed by `DeclIdx` (aliases, port counts, value slots) is laid out
    // in this order and never moves; only at the very end do we permute the
    // runtime tables into execution order.
    let mut nodes: Vec<KernelNode> = Vec::with_capacity(ir_macro.body.node_count());
    let mut decl_idx_of: HashMap<NodeId, DeclIdx> = HashMap::new();
    let mut aliases: Vec<String> = Vec::with_capacity(ir_macro.body.node_count());

    // Ensure that we only allow single kernels for the time being, perhaps multi in the future
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

        let mut params = ir_node.params.clone();
        substitute_templates(&mut params, &resolved_params);

        let node = build_kernel_node(&ir_node.node_type, rb, &DSLParams::new(&params))?;

        decl_idx_of.insert(ir_node.id, DeclIdx(nodes.len()));
        aliases.push(ir_node.alias.clone());
        nodes.push(node);
    }

    // Find port counts for correct size work buffer
    let in_counts: Vec<usize> = nodes.iter().map(|n| n.ports().audio_in.len()).collect();
    let out_counts: Vec<usize> = nodes.iter().map(|n| n.ports().audio_out.len()).collect();

    // Assign `values` slots node-by-node in declaration order. This layout
    // is final: reordering nodes later moves the *tables*, never the slots,
    // so `Src::Internal` references stay valid.
    let mut first_value_slot: Vec<ValueSlot> = Vec::with_capacity(nodes.len());
    let mut total_out = 0usize;
    for &c in &out_counts {
        first_value_slot.push(ValueSlot(total_out as u32));
        total_out += c;
    }
    let value_slot =
        |decl: DeclIdx, port: usize| ValueSlot(first_value_slot[decl.0].0 + port as u32);

    // Source lists are built nested per (node, port) — that is the easy shape
    // during resolution — and flattened into the runtime tables at the end.
    let mut sources_by_decl: Vec<Vec<Vec<Src>>> =
        in_counts.iter().map(|&c| vec![Vec::new(); c]).collect();

    // Interior edges
    let mut successors: Vec<Vec<DeclIdx>> = vec![Vec::new(); nodes.len()];

    for edge in ir_macro.body.edges() {
        if edge.source_selector != NodeSelector::Single
            || edge.sink_selector != NodeSelector::Single
        {
            return Err(unsupported("node selectors inside kernels".to_string()));
        }

        let src = decl_idx_of[&edge.source];
        let snk = decl_idx_of[&edge.sink];

        let src_ports = resolve_port(
            &edge.source_port,
            &nodes[src.0].ports().audio_out,
            &aliases[src.0],
        )?;
        let snk_ports = resolve_port(
            &edge.sink_port,
            &nodes[snk.0].ports().audio_in,
            &aliases[snk.0],
        )?;

        // broadcast, mirroring the block-rate builder.
        // zip when equal, replicate one source, or sum many sources into one port.
        let pairs: Vec<(usize, usize)> = match (src_ports.len(), snk_ports.len()) {
            (a, b) if a == b => src_ports.into_iter().zip(snk_ports).collect(),
            (1, _) => snk_ports.into_iter().map(|t| (src_ports[0], t)).collect(),
            (_, 1) => src_ports.into_iter().map(|s| (s, snk_ports[0])).collect(),
            (a, b) => {
                return Err(ValidationError::InvalidParameter(format!(
                    "kernel '{}': cannot match port arity {a}:{b} between '{}' and '{}'",
                    ir_macro.name, aliases[src.0], aliases[snk.0]
                )));
            }
        };

        for (src_port, snk_port) in pairs {
            sources_by_decl[snk.0][snk_port].push(Src::Internal(value_slot(src, src_port)));
        }
        successors[src.0].push(snk);
    }

    // Wire up the virtual inputs (these come from external nodes)
    for (ext_idx, (_name, targets)) in ir_macro.virtual_input_map.iter().enumerate() {
        for (node_id, _selector, port) in targets {
            let decl = decl_idx_of[node_id];
            let target_ports =
                resolve_port(port, &nodes[decl.0].ports().audio_in, &aliases[decl.0])?;
            for tp in target_ports {
                sources_by_decl[decl.0][tp].push(Src::External(ExternalInput(ext_idx as u32)));
            }
        }
    }

    // Topological order with cycles broken.
    let exec_order: Vec<DeclIdx> = execution_order(nodes.len(), &successors);

    // Where did each declared node end up?
    let mut exec_pos_of: Vec<ExecIdx> = vec![ExecIdx(0); nodes.len()];
    for (pos, &decl) in exec_order.iter().enumerate() {
        exec_pos_of[decl.0] = ExecIdx(pos);
    }

    // Flatten the nested source lists into the runtime tables, walking nodes
    // in execution order so a tick reads `port_sources`/`src_pool` front to
    // back with no pointer chasing.
    let mut layouts: Vec<NodeLayout> = Vec::with_capacity(nodes.len());
    let mut port_sources: Vec<(u32, u32)> = Vec::new();
    let mut src_pool: Vec<Src> = Vec::new();

    for &decl in &exec_order {
        layouts.push(NodeLayout {
            first_in_port: port_sources.len() as u32,
            n_in: in_counts[decl.0] as u32,
            first_value_slot: first_value_slot[decl.0].0,
            n_out: out_counts[decl.0] as u32,
        });
        for port_srcs in &sources_by_decl[decl.0] {
            port_sources.push((src_pool.len() as u32, port_srcs.len() as u32));
            src_pool.extend_from_slice(port_srcs);
        }
    }

    let mut node_pool: Vec<Option<KernelNode>> = nodes.into_iter().map(Some).collect();
    let nodes: Vec<KernelNode> = exec_order
        .iter()
        .map(|&decl| node_pool[decl.0].take().unwrap())
        .collect();

    let sink = decl_idx_of[&ir_macro.sink];

    let n_exterior_in = ir_macro.virtual_input_map.len();
    if n_exterior_in > MAX_FRAME_PORTS || out_counts[sink.0] > MAX_FRAME_PORTS {
        return Err(unsupported(format!(
            "kernel '{}' exceeds {MAX_FRAME_PORTS} exterior ports",
            ir_macro.name
        )));
    }

    // Just leak here for the time being, we may want to refactor this later and use owned Strings.
    let audio_in: Vec<PortMeta> = ir_macro
        .virtual_input_map
        .keys()
        .enumerate()
        .map(|(i, name)| PortMeta {
            name: Box::leak(name.clone().into_boxed_str()),
            index: i,
        })
        .collect();

    let sink_out_ports = nodes[exec_pos_of[sink.0].0].ports().audio_out.clone();
    let out_slots: Vec<ValueSlot> = (0..out_counts[sink.0])
        .map(|p| value_slot(sink, p))
        .collect();

    let max_in = in_counts.iter().copied().max().unwrap_or(0);

    Ok(KernelGraph {
        nodes,
        layouts: layouts.into_boxed_slice(),
        port_sources: port_sources.into_boxed_slice(),
        src_pool: src_pool.into_boxed_slice(),
        values: vec![0.0; total_out].into_boxed_slice(),
        out_slots: out_slots.into_boxed_slice(),
        scratch_in: vec![None; max_in].into_boxed_slice(),
        ports: Ports {
            audio_in,
            audio_out: sink_out_ports,
        },
    })
}

pub const EXAMPLE_PLATE_KERNEL_PATCH: &str = r#"
    // mod_range_l/r: LFO excursion of the two modulated tank allpasses, in
    // ms. Whole-array values template fine ($mod_range_l below); only
    // per-element templates inside an array literal do not.
    kernel plate(
        predelay = 10.0,
        bandwidth_a = 0.0005,
        damping = 0.3,
        decay = 0.5,
        wet = 0.3,
        dry = 0.7,
        mod_range_l = [22.3111, 22.8487],
        mod_range_r = [30.2408, 30.7784]
    ) {
        in in_l in_r

        audio {
            // input chain: mono sum -> predelay -> bandwidth one-pole -> 4 diffusers
            mult: mono { val: 0.5 },
            tap: pre { delay_length: $predelay, chans: 1 },
            onepole: bw { a: $bandwidth_a, chans: 1 },
            allpass: diff1 { delay_length: 4.7713, feedback: 0.75, chans: 1 },
            allpass: diff2 { delay_length: 3.5953, feedback: 0.75, chans: 1 },
            allpass: diff3 { delay_length: 12.7348, feedback: 0.625, chans: 1 },
            allpass: diff4 { delay_length: 9.3075, feedback: 0.625, chans: 1 },

            // LFO modulatings the first tank allpasses
            sine: lfo_l { freq: 0.7 },
            sine: lfo_r { freq: 0.7, phase: 0.25 },
            allpass: tank_ap_l { delay_length: 22.5799, feedback: -0.7, chans: 1 },
            allpass: tank_ap_r { delay_length: 30.5096, feedback: -0.7, chans: 1 },

            // main tank delays + damping + decay per branch
            tap: del_a { delay_length: 149.6254, chans: 1 },
            onepole: damp_l { a: $damping, chans: 1 },
            mult: decay_l { val: $decay },
            tap: del_b { delay_length: 124.9958, chans: 1 },

            tap: del_c { delay_length: 141.6955, chans: 1 },
            onepole: damp_r { a: $damping, chans: 1 },
            mult: decay_r { val: $decay },
            tap: del_d { delay_length: 106.28, chans: 1 },

            // second tank allpasses from primitives: w = in + 0.5 d, out = d - 0.5 w
            add: ap2_l_w { val: 0.0 },
            tap: ap2_l_d { delay_length: 60.4818, chans: 1 },
            mult: ap2_l_fb { val: 0.5 },
            mult: ap2_l_ff { val: -0.5 },
            add: ap2_l_out { val: 0.0 },

            add: ap2_r_w { val: 0.0 },
            tap: ap2_r_d { delay_length: 89.2444, chans: 1 },
            mult: ap2_r_fb { val: 0.5 },
            mult: ap2_r_ff { val: -0.5 },
            add: ap2_r_out { val: 0.0 },

            // output tap matrix: one tap node per read offset
            tap: yl1 { delay_length: 8.9379, chans: 1 },
            tap: yl2 { delay_length: 99.9295, chans: 1 },
            tap: yl3 { delay_length: 64.2788, chans: 1 },
            tap: yl4 { delay_length: 67.0676, chans: 1 },
            tap: yl5 { delay_length: 66.866, chans: 1 },
            tap: yl6 { delay_length: 6.2834, chans: 1 },
            tap: yl7 { delay_length: 35.8187, chans: 1 },

            tap: yr1 { delay_length: 11.8612, chans: 1 },
            tap: yr2 { delay_length: 121.8708, chans: 1 },
            tap: yr3 { delay_length: 41.2621, chans: 1 },
            tap: yr4 { delay_length: 89.8156, chans: 1 },
            tap: yr5 { delay_length: 70.9318, chans: 1 },
            tap: yr6 { delay_length: 11.2563, chans: 1 },
            tap: yr7 { delay_length: 4.0657, chans: 1 },

            mult: gl1 { val: 0.6 },
            mult: gl2 { val: 0.6 },
            mult: gl3 { val: -0.6 },
            mult: gl4 { val: 0.6 },
            mult: gl5 { val: -0.6 },
            mult: gl6 { val: -0.6 },
            mult: gl7 { val: -0.6 },

            mult: gr1 { val: 0.6 },
            mult: gr2 { val: 0.6 },
            mult: gr3 { val: -0.6 },
            mult: gr4 { val: 0.6 },
            mult: gr5 { val: -0.6 },
            mult: gr6 { val: -0.6 },
            mult: gr7 { val: -0.6 },

            mult: wet_l { val: $wet },
            mult: wet_r { val: $wet },
            mult: dry_l { val: $dry },
            mult: dry_r { val: $dry },
            add: out { val: 0.0, chans: 2 },
        }

        control {
            map: lfo_l_ms { range: [-1.0, 1.0], new_range: $mod_range_l },
            map: lfo_r_ms { range: [-1.0, 1.0], new_range: $mod_range_r },
        }

        // input chain
        in_l >> mono[0]
        in_r >> mono[0]
        mono >> pre[0] >> bw[0] >> diff1[0] >> diff2[0] >> diff3[0] >> diff4[0]

        // LFO -> ms -> modulated tank allpass delay
        lfo_l >> lfo_l_ms >> tank_ap_l.delay_length
        lfo_r >> lfo_r_ms >> tank_ap_r.delay_length

        // figure-eight: each branch takes the diffused input plus the other
        // branch's tail (this closes the tank cycle; one of the two returns
        // picks up the implicit z-1)
        diff4 >> tank_ap_l[0]
        del_d >> tank_ap_l[0]
        diff4 >> tank_ap_r[0]
        del_b >> tank_ap_r[0]

        // left branch
        tank_ap_l >> del_a[0]
        del_a >> damp_l[0] >> decay_l[0] >> ap2_l_w[0]
        ap2_l_fb >> ap2_l_w[0]
        ap2_l_w >> ap2_l_d[0]
        ap2_l_d >> ap2_l_fb[0]
        ap2_l_d >> ap2_l_out[0]
        ap2_l_w >> ap2_l_ff[0]
        ap2_l_ff >> ap2_l_out[0]
        ap2_l_out >> del_b[0]

        // right branch
        tank_ap_r >> del_c[0]
        del_c >> damp_r[0] >> decay_r[0] >> ap2_r_w[0]
        ap2_r_fb >> ap2_r_w[0]
        ap2_r_w >> ap2_r_d[0]
        ap2_r_d >> ap2_r_fb[0]
        ap2_r_d >> ap2_r_out[0]
        ap2_r_w >> ap2_r_ff[0]
        ap2_r_ff >> ap2_r_out[0]
        ap2_r_out >> del_d[0]

        // left output taps (source signal = the line each 480L tap reads)
        tank_ap_r >> yl1[0] >> gl1[0] >> wet_l[0]
        tank_ap_r >> yl2[0] >> gl2[0] >> wet_l[0]
        ap2_r_w   >> yl3[0] >> gl3[0] >> wet_l[0]
        ap2_r_out >> yl4[0] >> gl4[0] >> wet_l[0]
        tank_ap_l >> yl5[0] >> gl5[0] >> wet_l[0]
        ap2_l_w   >> yl6[0] >> gl6[0] >> wet_l[0]
        ap2_l_out >> yl7[0] >> gl7[0] >> wet_l[0]

        // right output taps
        tank_ap_l >> yr1[0] >> gr1[0] >> wet_r[0]
        tank_ap_l >> yr2[0] >> gr2[0] >> wet_r[0]
        ap2_l_w   >> yr3[0] >> gr3[0] >> wet_r[0]
        ap2_l_out >> yr4[0] >> gr4[0] >> wet_r[0]
        tank_ap_r >> yr5[0] >> gr5[0] >> wet_r[0]
        ap2_r_w   >> yr6[0] >> gr6[0] >> wet_r[0]
        ap2_r_out >> yr7[0] >> gr7[0] >> wet_r[0]

        // wet/dry into the stereo collector
        wet_l >> out[0]
        wet_r >> out[1]
        in_l >> dry_l[0]
        in_r >> dry_r[0]
        dry_l >> out[0]
        dry_r >> out[1]

        { out }
    }
"#;

pub const EXAMPLE_KARPLUS_KERNEL_PATCH: &str = r#"
    kernel karplus(
        damping = 0.5,
        decay = 0.99,
        pluck = 0.995
    ) {
        in gate freq

        audio {
            // --- excitation: gate edge -> windowed, gate-masked noise burst ---
            noise: exc_src {},
            onepole: gate_follow { a: $pluck, chans: 1 },
            sub: env { val: 0.0, chans: 1 },
            mult: burst { val: 1.0, chans: 1 },
            mult: exc { val: 1.0, chans: 1 },

            // --- tuning: freq (Hz) -> 1/freq (s) -> ms -> delay_length ---
            sine: one { freq: 0.0, phase: 0.25 },
            div: period_s { val: 220.0 },
            mult: period_ms { val: 1000.0 },

            // --- the string loop ---
            add: mix { val: 0.0, chans: 1 },
            tap: string { delay_length: 4.5, chans: 1, capacity: 48000 },
            onepole: loop_lp { a: $damping, chans: 1 },
            mult: fb { val: $decay },
        }

        // excitation: env = gate - slow_follow(gate); burst = noise * env; exc = burst * gate
        gate >> gate_follow[0]
        gate >> env[0]
        gate_follow >> env[1]
        exc_src >> burst[0]
        env >> burst[1]
        burst >> exc[0]
        gate >> exc[1]

        // tuning: 1.0 / freq -> * 1000 -> ms into the delay length port
        one >> period_s[0]
        freq >> period_s[1]
        period_s >> period_ms[0]
        period_ms >> string.delay_length

        // string loop: exc + tail -> delay -> loop lowpass -> feedback gain
        exc >> mix[0]
        fb  >> mix[1]
        mix >> string[0] >> loop_lp[0] >> fb[0]

        // fb >> mix closes the cycle -> the engine's implicit z-1 lives here.
        { string }
    }
"#;

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

    /// `map` (a control node) is kernel-capable: scaling works per-sample,
    /// e.g. for shaping a modulator inside a feedback structure.
    #[test]
    fn map_scales_inside_kernel() {
        let src = r#"
            kernel lfo_scaled() {
                in mod_in

                control {
                    map { range: [-1.0, 1.0], new_range: [0.0, 10.0] }
                }

                mod_in >> map

                { map }
            }
            audio { sine }
            { sine }
        "#;

        let def = kernel_def(src, "lfo_scaled");
        let mut kg = build(&def, Object::new()).unwrap();

        let mut out = [0.0f32];
        kg.tick(&[Some(-1.0)], &mut out);
        assert_eq!(out[0], 0.0);
        kg.tick(&[Some(0.0)], &mut out);
        assert_eq!(out[0], 5.0);
        kg.tick(&[Some(1.0)], &mut out);
        assert_eq!(out[0], 10.0);
    }

    /// The reference plate kernel (the dogfood) must parse, lower with its
    /// cycles broken, and tick without blowing up.
    #[test]
    fn plate_kernel_lowers_and_ticks() {
        let src = format!("{EXAMPLE_PLATE_KERNEL_PATCH} audio {{ sine }} {{ sine }}");
        let def = kernel_def(&src, "plate");
        let mut kg = build(&def, Object::new()).expect("plate kernel should lower");

        assert_eq!(PerSampleNode::ports(&kg).audio_in.len(), 2);
        assert_eq!(PerSampleNode::ports(&kg).audio_out.len(), 2);

        // Impulse in, then run the tank for a while: output stays finite and
        // the reverb tail is actually audible.
        let mut out = [0.0f32; 2];
        let mut energy = 0.0f32;
        for n in 0..48_000 {
            let x = if n == 0 { 1.0 } else { 0.0 };
            kg.tick(&[Some(x), Some(x)], &mut out);
            assert!(
                out[0].is_finite() && out[1].is_finite(),
                "plate tank blew up at sample {n}"
            );
            energy += out[0] * out[0] + out[1] * out[1];
        }
        assert!(energy > 1e-4, "plate tail was silent");
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
