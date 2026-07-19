//! Backend-independent resolution of a `kernel` declaration into a [`KernelPlan`].
//!
//! This module owns everything about a kernel that is *topology*: which nodes
//! exist, what feeds each input port, what order they run in, and which reads
//! cross a cycle and therefore see the previous sample. It deliberately owns
//! nothing about *execution* — it constructs no DSP state, allocates no delay
//! lines, and never sees a sample rate.
//!
//! That split exists so two backends can consume one plan:
//!
//! - [`KernelGraph`](crate::kernel::KernelGraph) — the interpreter, which packs
//!   the plan into flat runtime tables and walks them every sample. This is the
//!   backend runtime-authored DSL needs, since a string that arrives at runtime
//!   can never have been compiled.
//! - A future codegen backend, which consumes the same plan at macro-expansion
//!   time and emits straight-line Rust: one struct field per node, SSA locals
//!   instead of a value table, literal `+` instead of the accumulate loop. See
//!   `kernel_codegen.rs` for a hand-written example of the target shape.
//!
//! # The port oracle
//!
//! Resolution has one hard dependency on the node layer: it cannot resolve
//! `a >> b[cutoff]`, expand a bare `a >> b`, or match port arity without
//! knowing how many ports each node has and what they are named — and port
//! shape is param-dependent (`mult { chans: 8 }` is 8-in/8-out). Rather than
//! duplicate those arity rules in a table that would silently drift from the
//! node implementations, the resolver takes a [`PortOracle`] callback and asks.
//!
//! The oracle answers by *constructing* the node and reading its `ports()`,
//! then discarding it — see [`ProbeOracle`](crate::kernel::ProbeOracle). That
//! only works because node construction is pure, an invariant worth stating
//! because it has already been violated once: it must construct against a
//! **scratch** resource builder (`tap` allocates a delay line on construction,
//! so probing against the real builder would allocate every delay line twice),
//! and no constructor may draw from global state. See
//! [`PlanNode::identity_seed`].

use indexmap::IndexMap;

use crate::{
    builder::ValidationError,
    dsl::{
        expand::substitute_templates,
        ir::{DSLParams, IRMacro, IRNodeKind, NodeId, NodeSelector, Object, Port, Value},
    },
    persample::MAX_FRAME_PORTS,
    ports::{PortMeta, Ports},
};
use std::collections::HashMap;

/// A node's position in kernel-body declaration order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct DeclIdx(usize);

/// A node's position in the final execution order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ExecIdx(usize);

/// A slot in the kernel's value table — one per interior output port.
///
/// Slots are assigned in *declaration* order and never move. Execution order
/// permutes the node list, not the slots, so a slot reference stays valid
/// across the reorder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValueSlot(pub u32);

/// Where one summed contribution to an input port comes from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PlanSrc {
    /// The kernel's exterior input frame, by index. Unpatched at runtime means
    /// this contributes nothing and does not count toward "port is patched".
    Exterior(u32),
    /// Another interior node's output slot.
    Interior {
        slot: ValueSlot,
        /// True when this read crosses a back edge: the source node executes
        /// at or after the reader, so the value read is the source's output
        /// from the *previous* tick. This is the implicit z⁻¹ that makes
        /// feedback legal.
        ///
        /// The interpreter does not branch on this — its persistent value
        /// table gives previous-sample semantics for free. It exists for the
        /// codegen backend, which must read a `z_*` struct field here instead
        /// of an SSA local, and which has no value table to get it implicitly.
        delayed: bool,
    },
}

/// Derive a stable RNG seed for one node from its instantiation salt and alias.
///
/// This is an explicit FNV-1a rather than [`std::hash::DefaultHasher`] on
/// purpose. `DefaultHasher`'s output is explicitly not guaranteed stable across
/// Rust releases, and this value has to agree between two computations that may
/// happen in different compilations entirely: the interpreter derives it at
/// graph-build time, while codegen bakes it in as a literal at macro-expansion
/// time. If those two disagree, a kernel containing `noise` sounds different
/// depending on which backend built it, and the equivalence oracle silently
/// stops meaning anything. Do not swap this for a "better" hash.
pub fn identity_seed(salt: &str, alias: &str) -> u32 {
    let mut hash: u32 = 0x811C_9DC5;
    for byte in salt.bytes().chain(*b"::").chain(alias.bytes()) {
        hash ^= byte as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    // Callers feed xorshift-style generators, which stall on a zero state.
    hash | 1
}

/// One interior node param fed by a kernel-level param.
#[derive(Clone, Debug, PartialEq)]
pub struct ParamTarget {
    /// Alias of the interior node, e.g. `fb`.
    pub node_alias: String,
    /// The node's own param name, e.g. `val` or `freq` — what a
    /// [`SetParam`](crate::msg::NodeMessage::SetParam) message must carry.
    pub node_param: String,
}

/// A param the kernel declares in its signature, and everywhere it lands.
///
/// The kernel signature is the natural boundary for the structural-vs-runtime
/// split: `kernel modtap4(depth = 12.0, ...)` is precisely the set of knobs the
/// author chose to expose, so declared params become settable at runtime while
/// interior literals stay baked.
#[derive(Clone, Debug)]
pub struct PlanParam {
    /// Name as declared, e.g. `depth`.
    pub name: String,
    /// Declared default, used when an instantiation does not override it.
    pub default: Value,
    /// Every interior node param this feeds. One kernel param can drive
    /// several (a `$rate` shared by four LFOs, say), which is why routing has
    /// to fan out rather than assume a single destination.
    pub targets: Vec<ParamTarget>,
}

/// Params that determine a node's *shape* rather than its value, and so cannot
/// vary at runtime: they fix port arity and buffer allocation, which the
/// generated struct bakes in at compile time.
const STRUCTURAL_PARAMS: &[&str] = &["chans", "capacity"];

/// One node in a resolved plan.
#[derive(Clone, Debug)]
pub struct PlanNode {
    /// The alias from the kernel body. Codegen uses this for the struct field
    /// name; the interpreter only needs it for diagnostics.
    pub alias: String,
    /// DSL node type, e.g. `"sine"`. Resolved by `build_kernel_node`.
    pub node_type: String,
    /// Params with `$template` substitution already applied.
    pub params: Object,
    /// Stable RNG seed for nodes carrying random state (currently `noise`).
    ///
    /// Derived once here, by [`identity_seed`], so that both backends read the
    /// same number out of the plan rather than each deriving their own. Node
    /// construction must stay a pure function of params plus this seed —
    /// anything order-dependent (a global counter, say) makes generated code
    /// unable to reproduce the interpreter, and makes a kernel's sound depend
    /// on what else was built beforehand.
    pub identity_seed: u32,
    /// First value-table slot owned by this node (declaration-order layout).
    pub slot_base: ValueSlot,
    /// Number of output ports, i.e. slots owned starting at `slot_base`.
    pub n_out: usize,
    /// One entry per input port, each the list of contributions summed into
    /// that port. An empty list means unpatched — the node falls back to its
    /// internal param.
    pub inputs: Vec<Vec<PlanSrc>>,
}

impl PlanNode {
    /// Number of input ports.
    pub fn n_in(&self) -> usize {
        self.inputs.len()
    }
}

/// A fully resolved, backend-independent kernel.
#[derive(Clone, Debug)]
pub struct KernelPlan {
    /// Kernel name, for diagnostics and generated type names.
    pub name: String,
    /// Interior nodes in **execution order**. Their `slot_base` values remain
    /// in declaration order.
    pub nodes: Vec<PlanNode>,
    /// Exterior input port names, in frame order. Positionally matches the
    /// indices in [`PlanSrc::Exterior`].
    pub input_names: Vec<String>,
    /// Value slots exposed as the kernel's exterior outputs (the sink's).
    pub output_slots: Vec<ValueSlot>,
    /// Names of the exterior output ports, parallel to `output_slots`.
    pub output_names: Vec<String>,
    /// Total number of value slots — the size of the value table.
    pub total_slots: usize,
    /// Params the kernel declares, with every interior node param each feeds.
    ///
    /// Resolution normally substitutes `$templates` and discards where they
    /// came from; this preserves that provenance so codegen can emit setters
    /// and message routing. Empty for kernels that declare no params.
    pub params: Vec<PlanParam>,
}

impl KernelPlan {
    /// The exterior port signature. This is what the block-rate graph sees and
    /// what buffer allocation is sized from, so both backends must agree on it.
    ///
    /// Port names are leaked to satisfy `PortMeta`'s `&'static str`. A kernel is
    /// built once at graph-build time, so this is bounded by the patch, not by
    /// runtime. Codegen has no such problem — it emits real string literals.
    pub fn ports(&self) -> Ports {
        let meta = |names: &[String]| {
            names
                .iter()
                .enumerate()
                .map(|(index, name)| PortMeta {
                    name: Box::leak(name.clone().into_boxed_str()),
                    index,
                })
                .collect()
        };

        Ports {
            audio_in: meta(&self.input_names),
            audio_out: meta(&self.output_names),
        }
    }

    /// Names of the declared params, in declaration order.
    pub fn param_names(&self) -> Vec<&str> {
        self.params.iter().map(|p| p.name.as_str()).collect()
    }

    /// Widest input-port count across all nodes — the interpreter's scratch
    /// frame size.
    pub fn max_node_inputs(&self) -> usize {
        self.nodes.iter().map(PlanNode::n_in).max().unwrap_or(0)
    }
}

/// Answers "what ports does a node of this type, with these params, have?".
///
/// Implemented by constructing the node and reading `ports()`, so that arity
/// rules live in exactly one place — the node implementations themselves.
pub trait PortOracle {
    fn ports_for(&mut self, node_type: &str, params: &DSLParams) -> Result<Ports, ValidationError>;
}

impl<F> PortOracle for F
where
    F: FnMut(&str, &DSLParams) -> Result<Ports, ValidationError>,
{
    fn ports_for(&mut self, node_type: &str, params: &DSLParams) -> Result<Ports, ValidationError> {
        self(node_type, params)
    }
}

fn unsupported(what: impl Into<String>) -> ValidationError {
    ValidationError::UnsupportedInKernel(what.into())
}

/// Resolve a `Port` reference against a node's port list, yielding the port
/// indices it names.
///
/// Note `Port::None` expands to *every* port, matching block-rate builder
/// semantics: `a >> b` targets all of b's inputs, named modulation ports
/// included. Write `a >> b[0]` to hit only the first.
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
/// each tick, so the reader sees the source's value from the *previous* tick
/// — the implicit z⁻¹ that makes feedback loops legal. DFS roots are tried in
/// declaration order, so *which* edge of a cycle becomes the delayed one
/// follows the order nodes appear in the kernel body.
fn execution_order(node_count: usize, successors: &[Vec<DeclIdx>]) -> Vec<DeclIdx> {
    let mut state = vec![VisitState::Unvisited; node_count];
    let mut finish_order: Vec<DeclIdx> = Vec::with_capacity(node_count);

    // Explicit stack rather than recursion: kernel bodies are user input, and
    // a deep chain should not blow the native stack at graph-build time.
    for root in 0..node_count {
        if state[root] != VisitState::Unvisited {
            continue;
        }

        let mut stack: Vec<(DeclIdx, usize)> = vec![(DeclIdx(root), 0)];
        state[root] = VisitState::OnPath;

        while let Some((node, edge_cursor)) = stack.last_mut() {
            let node = *node;
            let succs = &successors[node.0];

            if *edge_cursor < succs.len() {
                let next = succs[*edge_cursor];
                *edge_cursor += 1;

                if state[next.0] == VisitState::Unvisited {
                    state[next.0] = VisitState::OnPath;
                    stack.push((next, 0));
                }
                // OnPath  => back edge, skip (this is where the z⁻¹ lands).
                // Finished => forward/cross edge, already ordered.
            } else {
                state[node.0] = VisitState::Finished;
                finish_order.push(node);
                stack.pop();
            }
        }
    }

    finish_order.reverse();
    finish_order
}

/// Resolve a kernel definition plus one instantiation's params into a
/// backend-independent [`KernelPlan`].
///
/// `oracle` supplies port shape per node type; see the module docs for why
/// this is a callback rather than a static table.
///
/// `instance_salt` distinguishes *this instantiation* from other instantiations
/// of the same kernel, and feeds [`identity_seed`]. Callers should pass the
/// alias of the instantiating node, which spawning already makes unique per
/// instance (`voice.0`, `voice.1`, …). Without it every polyphonic voice would
/// share one noise seed and excite identically.
pub fn resolve_plan(
    ir_macro: &IRMacro,
    instance_params: &Object,
    instance_salt: &str,
    oracle: &mut impl PortOracle,
) -> Result<KernelPlan, ValidationError> {
    // Instantiation params override the kernel's declared defaults.
    let mut resolved_params = ir_macro.default_params.clone().unwrap_or_default();
    for (k, v) in instance_params {
        resolved_params.insert(k.clone(), v.clone());
    }

    // Pass 1: walk the body in declaration order, resolving each node's params
    // and asking the oracle for its port shape. Everything indexed by
    // `DeclIdx` below is laid out in this order and never moves.
    let mut plan_nodes: Vec<PlanNode> = Vec::with_capacity(ir_macro.body.node_count());
    let mut node_ports: Vec<Ports> = Vec::with_capacity(ir_macro.body.node_count());
    let mut decl_idx_of: HashMap<NodeId, DeclIdx> = HashMap::new();
    let mut template_bindings: Vec<(String, ParamTarget)> = Vec::new();

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

        // Record where each `$template` lands *before* substitution erases it.
        // This provenance is what lets codegen emit a setter that reaches the
        // right interior node param.
        for (node_param, value) in &params {
            let Value::Template(template) = value else {
                continue;
            };
            let kernel_param = template.trim_start_matches('$').to_string();

            if STRUCTURAL_PARAMS.contains(&node_param.as_str()) {
                return Err(unsupported(format!(
                    "kernel '{}': param '{node_param}' on '{}' fixes port arity or \
                     allocation, so it must be a literal, not the template \
                     '${kernel_param}'",
                    ir_macro.name, ir_node.alias
                )));
            }

            template_bindings.push((
                kernel_param,
                ParamTarget {
                    node_alias: ir_node.alias.clone(),
                    node_param: node_param.clone(),
                },
            ));
        }

        substitute_templates(&mut params, &resolved_params);

        let ports = oracle.ports_for(&ir_node.node_type, &DSLParams::new(&params))?;

        decl_idx_of.insert(ir_node.id, DeclIdx(plan_nodes.len()));
        plan_nodes.push(PlanNode {
            identity_seed: identity_seed(instance_salt, &ir_node.alias),
            alias: ir_node.alias.clone(),
            node_type: ir_node.node_type.clone(),
            params,
            // Backfilled once all port counts are known.
            slot_base: ValueSlot(0),
            n_out: ports.audio_out.len(),
            inputs: vec![Vec::new(); ports.audio_in.len()],
        });
        node_ports.push(ports);
    }

    // Assign value slots in declaration order. This layout is final.
    // Bases are kept in their own vec so the `value_slot` helper below can be
    // used while `plan_nodes` is borrowed mutably at some other index.
    let mut slot_bases: Vec<ValueSlot> = Vec::with_capacity(plan_nodes.len());
    let mut total_slots = 0usize;
    for node in &mut plan_nodes {
        node.slot_base = ValueSlot(total_slots as u32);
        slot_bases.push(node.slot_base);
        total_slots += node.n_out;
    }
    let value_slot = |decl: DeclIdx, port: usize| ValueSlot(slot_bases[decl.0].0 + port as u32);

    // Pass 2: interior edges.
    let mut successors: Vec<Vec<DeclIdx>> = vec![Vec::new(); plan_nodes.len()];
    // Recorded as (sink, port, position-in-list, source-decl) so pass 4 can
    // mark which reads ended up crossing a back edge.
    let mut interior_edges: Vec<(DeclIdx, usize, usize, DeclIdx)> = Vec::new();

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
            &node_ports[src.0].audio_out,
            &plan_nodes[src.0].alias,
        )?;
        let snk_ports = resolve_port(
            &edge.sink_port,
            &node_ports[snk.0].audio_in,
            &plan_nodes[snk.0].alias,
        )?;

        // Broadcast, mirroring the block-rate builder: zip when equal,
        // replicate one source, or sum many sources into one port.
        let pairs: Vec<(usize, usize)> = match (src_ports.len(), snk_ports.len()) {
            (a, b) if a == b => src_ports.into_iter().zip(snk_ports).collect(),
            (1, _) => snk_ports.into_iter().map(|t| (src_ports[0], t)).collect(),
            (_, 1) => src_ports.into_iter().map(|s| (s, snk_ports[0])).collect(),
            (a, b) => {
                return Err(ValidationError::InvalidParameter(format!(
                    "kernel '{}': cannot match port arity {a}:{b} between '{}' and '{}'",
                    ir_macro.name, plan_nodes[src.0].alias, plan_nodes[snk.0].alias
                )));
            }
        };

        for (src_port, snk_port) in pairs {
            let slot = value_slot(src, src_port);
            let position = plan_nodes[snk.0].inputs[snk_port].len();
            plan_nodes[snk.0].inputs[snk_port].push(PlanSrc::Interior {
                slot,
                // Backfilled in pass 4, once execution order is known.
                delayed: false,
            });
            interior_edges.push((snk, snk_port, position, src));
        }
        successors[src.0].push(snk);
    }

    // Pass 3: exterior inputs (the `in` declarations / virtual input map).
    for (ext_idx, (_name, targets)) in ir_macro.virtual_input_map.iter().enumerate() {
        for (node_id, _selector, port) in targets {
            let decl = decl_idx_of[node_id];
            let target_ports = resolve_port(
                port,
                &node_ports[decl.0].audio_in,
                &plan_nodes[decl.0].alias,
            )?;
            for tp in target_ports {
                plan_nodes[decl.0].inputs[tp].push(PlanSrc::Exterior(ext_idx as u32));
            }
        }
    }

    // Pass 4: order the graph, then mark delayed reads.
    let exec_order = execution_order(plan_nodes.len(), &successors);

    let mut exec_pos_of: Vec<ExecIdx> = vec![ExecIdx(0); plan_nodes.len()];
    for (pos, &decl) in exec_order.iter().enumerate() {
        exec_pos_of[decl.0] = ExecIdx(pos);
    }

    // A read is delayed exactly when its source does not run strictly before
    // its reader this tick — including self-loops, where src == snk.
    for (snk, port, position, src) in interior_edges {
        let delayed = exec_pos_of[src.0].0 >= exec_pos_of[snk.0].0;
        if let PlanSrc::Interior { delayed: d, .. } = &mut plan_nodes[snk.0].inputs[port][position]
        {
            *d = delayed;
        }
    }

    // Exterior signature. Outputs are the sink node's output ports.
    let sink = decl_idx_of[&ir_macro.sink];
    let sink_out = &node_ports[sink.0].audio_out;

    let n_exterior_in = ir_macro.virtual_input_map.len();
    if n_exterior_in > MAX_FRAME_PORTS || sink_out.len() > MAX_FRAME_PORTS {
        return Err(unsupported(format!(
            "kernel '{}' exceeds {MAX_FRAME_PORTS} exterior ports",
            ir_macro.name
        )));
    }

    let output_slots: Vec<ValueSlot> = (0..sink_out.len()).map(|p| value_slot(sink, p)).collect();
    let output_names: Vec<String> = sink_out.iter().map(|p| p.name.to_string()).collect();
    let input_names: Vec<String> = virtual_input_names(&ir_macro.virtual_input_map);

    // Finally permute the node list into execution order. Slot bases stay put,
    // so every `PlanSrc::Interior` reference survives the reorder.
    let mut pool: Vec<Option<PlanNode>> = plan_nodes.into_iter().map(Some).collect();
    let nodes: Vec<PlanNode> = exec_order
        .iter()
        .map(|&decl| pool[decl.0].take().expect("each node moved exactly once"))
        .collect();

    // Group provenance by declared param, keeping declaration order. A param
    // declared but never referenced still appears, with no targets — setting it
    // is a no-op rather than an error, matching how the interpreter treats an
    // unused default.
    let params: Vec<PlanParam> = ir_macro
        .default_params
        .as_ref()
        .map(|declared| {
            declared
                .iter()
                .map(|(name, declared_default)| PlanParam {
                    name: name.clone(),
                    // The *resolved* value, not the declared default: an
                    // instantiation may override it, and construction already
                    // used the override. A field seeded from the declared
                    // default would misreport the node's actual state.
                    default: resolved_params
                        .get(name)
                        .unwrap_or(declared_default)
                        .clone(),
                    targets: template_bindings
                        .iter()
                        .filter(|(param, _)| param == name)
                        .map(|(_, target)| target.clone())
                        .collect(),
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(KernelPlan {
        name: ir_macro.name.clone(),
        params,
        nodes,
        input_names,
        output_slots,
        output_names,
        total_slots,
    })
}

fn virtual_input_names(map: &IndexMap<String, Vec<(NodeId, NodeSelector, Port)>>) -> Vec<String> {
    map.keys().cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{BlockSize, Config},
        dsl::{lower::ast_to_graph, parse::legato_parser},
        kernel::ProbeOracle,
    };

    /// Resolve `src`'s kernel `name` into a plan, using the real port oracle.
    fn plan_of(src: &str, name: &str, salt: &str) -> KernelPlan {
        let ast = legato_parser(src).expect("kernel test source should parse");
        let graph = ast_to_graph(ast);
        let def = graph
            .macro_registry
            .get(name)
            .unwrap_or_else(|| panic!("kernel '{name}' missing from registry"))
            .clone();

        let config = Config::new(48_000, BlockSize::Block64, 1, 0);
        resolve_plan(&def, &Object::new(), salt, &mut ProbeOracle::new(&config))
            .expect("kernel should resolve")
    }

    /// `add -> mult -> add` is a two-node cycle. Whichever edge the DFS
    /// classifies as the back edge must be the one — and the only one —
    /// flagged `delayed`.
    ///
    /// The interpreter never reads this flag (its persistent value table gives
    /// previous-sample semantics implicitly), so nothing else in the suite
    /// would notice it being wrong. Codegen depends on it entirely: a delayed
    /// read has to come from a `z_*` field rather than an SSA local, and
    /// getting it backwards silently changes the loop's transfer function.
    #[test]
    fn back_edge_is_the_only_delayed_read() {
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

        let plan = plan_of(src, "fb_loop", "inst");

        let mut delayed: Vec<(&str, usize)> = Vec::new();
        let mut immediate: Vec<(&str, usize)> = Vec::new();
        for node in &plan.nodes {
            for (port, srcs) in node.inputs.iter().enumerate() {
                for src in srcs {
                    if let PlanSrc::Interior { delayed: d, .. } = src {
                        if *d {
                            delayed.push((&node.alias, port));
                        } else {
                            immediate.push((&node.alias, port));
                        }
                    }
                }
            }
        }

        // `mult -> add[1]` closes the cycle, so add's port 1 is the delayed
        // read; `add -> mult[0]` runs same-tick.
        assert_eq!(
            delayed,
            vec![("add", 1)],
            "expected exactly the back edge to be delayed"
        );
        assert_eq!(
            immediate,
            vec![("mult", 0)],
            "expected the forward edge to be same-tick"
        );

        // And the ordering that makes that true: add runs before mult.
        let order: Vec<&str> = plan.nodes.iter().map(|n| n.alias.as_str()).collect();
        assert_eq!(order, vec!["add", "mult"]);
    }

    /// A purely feed-forward kernel must have no delayed reads at all.
    #[test]
    fn acyclic_kernel_has_no_delayed_reads() {
        let src = r#"
            kernel chain() {
                in audio_in

                audio {
                    mult: a { val: 2.0 },
                    mult: b { val: 3.0 }
                }

                audio_in >> a[0]
                a >> b[0]

                { b }
            }
            audio { sine }
            { sine }
        "#;

        let plan = plan_of(src, "chain", "inst");
        let any_delayed = plan
            .nodes
            .iter()
            .flat_map(|n| n.inputs.iter())
            .flatten()
            .any(|src| matches!(src, PlanSrc::Interior { delayed: true, .. }));
        assert!(!any_delayed, "feed-forward kernel should have no z⁻¹");
    }

    /// A param that fixes port arity cannot be a runtime knob: the generated
    /// struct bakes arity in at compile time, so a template there has to be
    /// rejected at resolution rather than silently producing a node whose
    /// channel count disagrees with its wiring.
    #[test]
    fn structural_params_reject_templates() {
        let src = r#"
            kernel bad(n = 4.0) {
                in audio_in

                audio {
                    mult: wide { val: 1.0, chans: $n }
                }

                audio_in >> wide[0]

                { wide }
            }
            audio { sine }
            { sine }
        "#;

        let ast = legato_parser(src).expect("should parse");
        let def = ast_to_graph(ast)
            .macro_registry
            .get("bad")
            .expect("kernel")
            .clone();
        let config = Config::new(48_000, BlockSize::Block64, 1, 0);

        match resolve_plan(&def, &Object::new(), "bad", &mut ProbeOracle::new(&config)) {
            Err(ValidationError::UnsupportedInKernel(msg)) => assert!(
                msg.contains("chans") && msg.contains("$n"),
                "error should name the param and the template: {msg}"
            ),
            other => panic!("expected UnsupportedInKernel, got {other:?}"),
        }
    }

    /// Provenance must survive substitution, and must fan out: one kernel param
    /// commonly drives several interior nodes, and a setter that reached only
    /// the first would leave the rest stale.
    #[test]
    fn declared_params_record_every_target() {
        let src = r#"
            kernel shared(rate = 2.0) {
                in audio_in

                audio {
                    sine: a { freq: $rate },
                    sine: b { freq: $rate },
                    add: mix { val: 0.0 }
                }

                audio_in >> mix[0]
                a >> mix[0]
                b >> mix[0]

                { mix }
            }
            audio { sine }
            { sine }
        "#;

        let plan = plan_of(src, "shared", "inst");

        assert_eq!(plan.params.len(), 1);
        let rate = &plan.params[0];
        assert_eq!(rate.name, "rate");

        let mut targets: Vec<(&str, &str)> = rate
            .targets
            .iter()
            .map(|t| (t.node_alias.as_str(), t.node_param.as_str()))
            .collect();
        targets.sort_unstable();

        assert_eq!(targets, vec![("a", "freq"), ("b", "freq")]);
    }

    const NOISE_SRC: &str = r#"
        kernel noisy() {
            in audio_in

            audio {
                noise: n1,
                noise: n2,
                add: mix { val: 0.0 }
            }

            audio_in >> mix[0]
            n1 >> mix[0]
            n2 >> mix[0]

            { mix }
        }
        audio { sine }
        { sine }
    "#;

    /// Seeds must be stable across resolutions, distinct between nodes, and
    /// distinct between instantiations — the last is what stops every poly
    /// voice from being excited by an identical noise stream.
    #[test]
    fn identity_seeds_are_stable_distinct_and_per_instance() {
        let seeds = |salt: &str| -> Vec<u32> {
            plan_of(NOISE_SRC, "noisy", salt)
                .nodes
                .iter()
                .filter(|n| n.node_type == "noise")
                .map(|n| n.identity_seed)
                .collect()
        };

        let voice0 = seeds("voice.0");
        let voice1 = seeds("voice.1");

        assert_eq!(voice0.len(), 2, "expected two noise nodes");
        // Stable: resolving the same kernel twice gives the same seeds.
        assert_eq!(voice0, seeds("voice.0"));
        // Distinct per node within one instantiation.
        assert_ne!(voice0[0], voice0[1]);
        // Distinct per instantiation, node for node.
        assert_ne!(voice0[0], voice1[0]);
        assert_ne!(voice0[1], voice1[1]);
        // Never zero: xorshift stalls on a zero state.
        assert!(voice0.iter().chain(&voice1).all(|&s| s != 0));
    }

    /// Regression: resolving a plan probes every node's ports by constructing
    /// it and throwing it away. That probe must be unobservable. It previously
    /// was not — `noise` drew from a global counter, so probing burned a seed
    /// and the real node came out different, retuning a Karplus string by most
    /// of an octave. Construction has to stay a pure function of its inputs.
    #[test]
    fn probing_ports_does_not_perturb_node_construction() {
        let before = plan_of(NOISE_SRC, "noisy", "voice.0");

        // Resolve some unrelated noise-bearing kernels in between; if any
        // global construction state existed, these would shift the seeds.
        for _ in 0..3 {
            let _ = plan_of(NOISE_SRC, "noisy", "other");
        }

        let after = plan_of(NOISE_SRC, "noisy", "voice.0");

        let seeds =
            |p: &KernelPlan| -> Vec<u32> { p.nodes.iter().map(|n| n.identity_seed).collect() };
        assert_eq!(
            seeds(&before),
            seeds(&after),
            "seeds must not depend on what else was built"
        );
    }
}
