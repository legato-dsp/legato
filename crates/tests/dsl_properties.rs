//! Property tests for the DSL pipeline: lower -> expand -> spawn -> resolve.
//!
//! Generated programs are real [`Ast`]s. Values that need no context
//! (`NodeSelector`, `Port`, `Value`) are drawn directly; the composites are built
//! afterwards, because `Ast` addresses nodes by name while a generator addresses
//! them by position. Aliases are derived from position, so they are unique.

use legato::builder::ValidationError;
use legato::dsl::ir::*;
use legato::dsl::parse::legato_parser;
use legato::dsl::pipeline::Pipeline;
use proptest::prelude::*;
use std::collections::BTreeSet;

const NAMESPACE: &str = "audio";
const LEAF_TYPE: &str = "leaf";
/// Endpoint port names; some hit a patch's virtual ports, some deliberately don't.
const PORT_NAMES: [&str; 3] = ["v0", "v1", "sig"];

/// `(drop the alias, instantiate a patch, which one, instance count, params)`.
type DeclSpec = (bool, bool, usize, u32, Object);
/// A connection with positional endpoints, named when the declarations exist.
type WireSpec = (usize, usize, NodeSelector, Port, NodeSelector, Port);
/// `(defaults, virtual ports, body, wiring, virtual-port targets, sink)`.
type MacroSpec = (
    usize,
    usize,
    Vec<DeclSpec>,
    Vec<WireSpec>,
    Vec<usize>,
    usize,
);

// ---------------------------------------------------------------------------
// Build
// ---------------------------------------------------------------------------

/// Mirrors the fallback in `lower::ast_to_graph`.
fn alias_of(decl: &NodeDeclaration) -> &str {
    decl.alias.as_deref().unwrap_or(&decl.node_type)
}

fn broadcastable(src: usize, snk: usize) -> bool {
    src == snk || src == 1 || snk == 1
}

fn clamp(sel: &mut NodeSelector, count: u32) {
    let count = count as usize;
    match sel {
        NodeSelector::Single | NodeSelector::All => {}
        NodeSelector::Index(i) => *i %= count,
        NodeSelector::Range(a, b) => {
            *a %= count;
            *b = *a + 1 + *b % (count - *a);
        }
    }
}

fn build_decls(specs: Vec<DeclSpec>, macros: &[AstMacro], prefix: &str) -> Vec<NodeDeclaration> {
    let bare: Vec<bool> = specs.iter().map(|(bare, ..)| *bare).collect();
    let mut decls: Vec<NodeDeclaration> = specs
        .into_iter()
        .enumerate()
        .map(
            |(i, (_, want_patch, pick, count, params))| NodeDeclaration {
                node_type: match want_patch && !macros.is_empty() {
                    true => macros[pick % macros.len()].name.clone(),
                    false => LEAF_TYPE.to_string(),
                },
                alias: Some(format!("{prefix}{i}")),
                params: Some(params),
                count,
            },
        )
        .collect();

    // An unaliased declaration takes its node type as alias, so it is only
    // unique where that type is declared once.
    for i in 0..decls.len() {
        let ty = decls[i].node_type.clone();
        if bare[i] && decls.iter().filter(|d| d.node_type == ty).count() == 1 {
            decls[i].alias = None;
        }
    }
    decls
}

fn build_conns(specs: Vec<WireSpec>, decls: &[NodeDeclaration]) -> Vec<Connection> {
    let n = decls.len();
    if n < 2 {
        return Vec::new();
    }
    specs
        .into_iter()
        .filter_map(|(a, b, mut src_sel, src_port, mut snk_sel, snk_port)| {
            // Distinct endpoints ordered forward, so the graph stays acyclic.
            let i = a % n;
            let j = (i + 1 + b % (n - 1)) % n;
            let (src, snk) = (&decls[i.min(j)], &decls[i.max(j)]);
            clamp(&mut src_sel, src.count);
            clamp(&mut snk_sel, snk.count);

            // Selected arity must match exactly, patch or leaf alike.
            if !broadcastable(
                src_sel.selected_count(src.count as usize),
                snk_sel.selected_count(snk.count as usize),
            ) {
                // Fan-in is legal for any source arity.
                snk_sel = NodeSelector::Index(0);
            }

            Some(Connection {
                source: Endpoint {
                    node: alias_of(src).to_string(),
                    node_selector: src_sel,
                    port: src_port,
                },
                sink: Endpoint {
                    node: alias_of(snk).to_string(),
                    node_selector: snk_sel,
                    port: snk_port,
                },
            })
        })
        .collect()
}

fn leaf(alias: &str, count: u32) -> NodeDeclaration {
    NodeDeclaration {
        node_type: LEAF_TYPE.to_string(),
        alias: Some(alias.to_string()),
        params: Some(Object::new()),
        count,
    }
}

fn scope(declarations: Vec<NodeDeclaration>) -> DeclarationScope {
    DeclarationScope {
        namespace: NAMESPACE.to_string(),
        declarations,
    }
}

fn endpoint(node: &str, port: Port) -> Endpoint {
    Endpoint {
        node: node.to_string(),
        node_selector: NodeSelector::Single,
        port,
    }
}

fn wire(src: &str, src_port: Port, snk: &str, snk_port: Port) -> Connection {
    Connection {
        source: endpoint(src, src_port),
        sink: endpoint(snk, snk_port),
    }
}

fn build_macro(idx: usize, spec: MacroSpec, earlier: &[AstMacro]) -> AstMacro {
    let (defaults, vports, decl_specs, wire_specs, vconn_specs, sink) = spec;
    let mut body = build_decls(decl_specs, earlier, "b");
    let sink = sink % body.len();
    // A single-instance sink bounds the arity of every edge out of the patch.
    body[sink].count = 1;

    let mut connections = Vec::new();
    for (i, target) in vconn_specs.into_iter().enumerate() {
        if vports == 0 {
            break;
        }
        let target = target % body.len();
        // Same for virtual-port targets: fan-in to one instance always broadcasts.
        body[target].count = 1;
        connections.push(wire(
            &format!("v{}", i % vports),
            Port::None,
            alias_of(&body[target]),
            Port::None,
        ));
    }
    connections.extend(build_conns(wire_specs, &body));

    AstMacro {
        name: format!("p{idx}"),
        kind: MacroKind::Patch,
        default_params: Some(
            (0..defaults)
                .map(|i| (format!("d{i}"), Value::F32(i as f32 + 1.0)))
                .collect(),
        ),
        virtual_ports_in: (0..vports).map(|i| format!("v{i}")).collect(),
        sink: alias_of(&body[sink]).to_string(),
        declarations: vec![DeclarationScope {
            namespace: NAMESPACE.to_string(),
            declarations: body,
        }],
        connections,
    }
}

// ---------------------------------------------------------------------------
// Transparency: one body expressed as a patch instance and inlined
// ---------------------------------------------------------------------------

const EXT: &str = "ext";
const OUT: &str = "out";
const INSTANCE: &str = "a0";
/// A default deliberately shadowed by an instantiation param.
const SHADOWED: Value = Value::F32(-999.0);

/// `(feed it, interior targets, give the source its targets' instance count)`.
type VPortSpec = (bool, Vec<usize>, bool);

#[derive(Debug, Clone)]
struct Transparency {
    patched: Ast,
    inlined: Ast,
}

/// A virtual port: the interior nodes it fans to, and the outside node feeding it.
struct VPort {
    name: String,
    targets: Vec<usize>,
    source: Option<NodeDeclaration>,
}

/// Resolve raw virtual-port draws against the body they address.
fn build_vports(specs: Vec<VPortSpec>, body: &[NodeDeclaration]) -> Vec<VPort> {
    specs
        .into_iter()
        .enumerate()
        .map(|(k, (fed, raw_targets, share_count))| {
            let mut targets: Vec<usize> = raw_targets.into_iter().map(|t| t % body.len()).collect();
            targets.sort_unstable();
            targets.dedup();

            // The source broadcasts onto every target, so it is either single or
            // matches a count they all share.
            let counts: BTreeSet<u32> = targets.iter().map(|&t| body[t].count).collect();
            let count = match (share_count, counts.len()) {
                (true, 1) => *counts.first().expect("checked non-empty"),
                _ => 1,
            };

            // Feeding a virtual port with no targets falls through to the patch
            // sink, which inlining has no way to express.
            let source = (fed && !targets.is_empty()).then(|| leaf(&format!("{EXT}{k}"), count));

            VPort {
                name: format!("v{k}"),
                targets,
                source,
            }
        })
        .collect()
}

/// Rewrite literal params into templates bound to the *same* value, so the two
/// forms must agree exactly. `picks` is `(templatise, bind via instantiation)`.
fn bind_templates(body: &mut [NodeDeclaration], picks: &[(bool, bool)]) -> (Object, Object) {
    let (mut defaults, mut overrides) = (Object::new(), Object::new());
    if picks.is_empty() {
        return (defaults, overrides);
    }
    let mut n = 0;
    for decl in body.iter_mut() {
        let Some(params) = decl.params.as_mut() else {
            continue;
        };
        for val in params.values_mut() {
            let (templatise, via_instance) = picks[n % picks.len()];
            n += 1;
            if !templatise {
                continue;
            }
            let name = format!("t{n}");
            let bound = std::mem::replace(val, Value::Template(format!("${name}")));
            match via_instance {
                // An instantiation param must beat the default it shadows.
                true => {
                    defaults.insert(name.clone(), SHADOWED);
                    overrides.insert(name, bound);
                }
                false => {
                    defaults.insert(name, bound);
                }
            }
        }
    }
    (defaults, overrides)
}

fn build_transparency(
    macro_specs: Vec<MacroSpec>,
    decl_specs: Vec<DeclSpec>,
    wire_specs: Vec<WireSpec>,
    vport_specs: Vec<VPortSpec>,
    sink: usize,
    downstream: bool,
    picks: Vec<(bool, bool)>,
) -> Transparency {
    let mut macros: Vec<AstMacro> = Vec::new();
    for (i, spec) in macro_specs.into_iter().enumerate() {
        let mac = build_macro(i, spec, &macros);
        macros.push(mac);
    }

    let mut body = build_decls(decl_specs, &macros, "b");
    // Templates are introduced deliberately below, so start from literals only.
    for decl in &mut body {
        for val in decl.params.iter_mut().flat_map(|p| p.values_mut()) {
            if matches!(val, Value::Template(_)) {
                *val = Value::F32(1.0);
            }
        }
    }

    let sink = sink % body.len();
    let sink_alias = alias_of(&body[sink]).to_string();
    let vports = build_vports(vport_specs, &body);
    let conns = build_conns(wire_specs, &body);

    // The inlined form keeps the literal values the templates are bound to.
    let inline_body = body.clone();
    let (defaults, overrides) = bind_templates(&mut body, &picks);

    // Interior wiring: each virtual port fans to its targets.
    let mut patch_conns: Vec<Connection> = vports
        .iter()
        .flat_map(|v| {
            v.targets
                .iter()
                .map(|&t| wire(&v.name, Port::None, alias_of(&inline_body[t]), Port::None))
        })
        .collect();
    patch_conns.extend(conns.clone());

    let name = format!("p{}", macros.len());
    let mut patched_macros = macros.clone();
    patched_macros.push(AstMacro {
        name: name.clone(),
        kind: MacroKind::Patch,
        default_params: Some(defaults),
        virtual_ports_in: vports.iter().map(|v| v.name.clone()).collect(),
        declarations: vec![scope(body)],
        connections: patch_conns,
        sink: sink_alias.clone(),
    });

    let sources: Vec<&NodeDeclaration> = vports.iter().filter_map(|v| v.source.as_ref()).collect();
    let top_sink = match downstream {
        true => OUT.to_string(),
        false => INSTANCE.to_string(),
    };

    // Patched: every source feeds the instance through the named virtual port.
    let mut patched_decls: Vec<NodeDeclaration> = sources.iter().map(|d| (*d).clone()).collect();
    patched_decls.push(NodeDeclaration {
        node_type: name,
        alias: Some(INSTANCE.to_string()),
        params: Some(overrides),
        count: 1,
    });
    let mut patched_conns: Vec<Connection> = vports
        .iter()
        .filter_map(|v| v.source.as_ref().map(|s| (v, s)))
        .map(|(v, s)| {
            wire(
                alias_of(s),
                Port::None,
                INSTANCE,
                Port::Named(v.name.clone()),
            )
        })
        .collect();
    if downstream {
        patched_decls.push(leaf(OUT, 1));
        patched_conns.push(wire(INSTANCE, Port::None, OUT, Port::None));
    }

    // Inlined: the same sources feed each interior target directly.
    let mut inline_decls: Vec<NodeDeclaration> = sources.iter().map(|d| (*d).clone()).collect();
    inline_decls.extend(inline_body.iter().cloned());
    let mut inline_conns: Vec<Connection> = vports
        .iter()
        .filter_map(|v| v.source.as_ref().map(|s| (v, s)))
        .flat_map(|(v, s)| {
            v.targets.iter().map(|&t| {
                wire(
                    alias_of(s),
                    Port::None,
                    alias_of(&inline_body[t]),
                    Port::None,
                )
            })
        })
        .collect();
    inline_conns.extend(conns);
    if downstream {
        inline_decls.push(leaf(OUT, 1));
        inline_conns.push(wire(&sink_alias, Port::None, OUT, Port::None));
    }

    Transparency {
        patched: Ast {
            declarations: vec![scope(patched_decls)],
            connections: patched_conns,
            macros: patched_macros,
            sink: top_sink.clone(),
            source: None,
        },
        inlined: Ast {
            declarations: vec![scope(inline_decls)],
            connections: inline_conns,
            macros,
            sink: match downstream {
                true => top_sink,
                false => sink_alias,
            },
            source: None,
        },
    }
}

// ---------------------------------------------------------------------------
// Multiplicity: one subject spawned `* n` versus declared n times
// ---------------------------------------------------------------------------

const SUBJECT: &str = "a";
const SRC: &str = "src";
const SNK: &str = "snk";

#[derive(Debug, Clone)]
struct Multiplicity {
    spawned: Ast,
    declared: Ast,
}

/// The instances a selector picks out of `n`.
fn selected(sel: &NodeSelector, n: u32) -> Vec<usize> {
    match sel {
        NodeSelector::Single | NodeSelector::All => (0..n as usize).collect(),
        NodeSelector::Index(i) => vec![*i],
        NodeSelector::Range(a, b) => (*a..*b).collect(),
    }
}

fn at(node: &str, sel: NodeSelector) -> Endpoint {
    Endpoint {
        node: node.to_string(),
        node_selector: sel,
        port: Port::None,
    }
}

fn build_multiplicity(
    macro_specs: Vec<MacroSpec>,
    use_patch: bool,
    n: u32,
    params: Object,
    upstream: Option<NodeSelector>,
    downstream: Option<NodeSelector>,
) -> Multiplicity {
    let mut macros: Vec<AstMacro> = Vec::new();
    for (i, spec) in macro_specs.into_iter().enumerate() {
        let mac = build_macro(i, spec, &macros);
        macros.push(mac);
    }

    // The last macro is the deepest, so a patch subject nests as far as possible.
    let node_type = match use_patch && !macros.is_empty() {
        true => macros.last().expect("checked non-empty").name.clone(),
        false => LEAF_TYPE.to_string(),
    };
    let subject = |alias: &str, count: u32| NodeDeclaration {
        node_type: node_type.clone(),
        alias: Some(alias.to_string()),
        params: Some(params.clone()),
        count,
    };

    let clamped = |sel: Option<NodeSelector>| {
        sel.map(|mut s| {
            clamp(&mut s, n);
            s
        })
    };
    let (upstream, downstream) = (clamped(upstream), clamped(downstream));

    let mut spawn_decls = Vec::new();
    let mut declare_decls = Vec::new();
    let (mut spawn_conns, mut declare_conns) = (Vec::new(), Vec::new());

    if let Some(sel) = &upstream {
        spawn_decls.push(leaf(SRC, 1));
        declare_decls.push(leaf(SRC, 1));
        spawn_conns.push(Connection {
            source: at(SRC, NodeSelector::Single),
            sink: at(SUBJECT, sel.clone()),
        });
        declare_conns.extend(
            selected(sel, n)
                .into_iter()
                .map(|i| wire(SRC, Port::None, &format!("{SUBJECT}{i}"), Port::None)),
        );
    }

    spawn_decls.push(subject(SUBJECT, n));
    declare_decls.extend((0..n).map(|i| subject(&format!("{SUBJECT}{i}"), 1)));

    if let Some(sel) = &downstream {
        spawn_decls.push(leaf(SNK, 1));
        declare_decls.push(leaf(SNK, 1));
        spawn_conns.push(Connection {
            source: at(SUBJECT, sel.clone()),
            sink: at(SNK, NodeSelector::Single),
        });
        declare_conns.extend(
            selected(sel, n)
                .into_iter()
                .map(|i| wire(&format!("{SUBJECT}{i}"), Port::None, SNK, Port::None)),
        );
    }

    // Spawning binds the sink to the last instance, so the declared form names it.
    let (spawn_sink, declare_sink) = match downstream.is_some() {
        true => (SNK.to_string(), SNK.to_string()),
        false => (SUBJECT.to_string(), format!("{SUBJECT}{}", n - 1)),
    };

    Multiplicity {
        spawned: Ast {
            declarations: vec![scope(spawn_decls)],
            connections: spawn_conns,
            macros: macros.clone(),
            sink: spawn_sink,
            source: None,
        },
        declared: Ast {
            declarations: vec![scope(declare_decls)],
            connections: declare_conns,
            macros,
            sink: declare_sink,
            source: None,
        },
    }
}

/// Graph shape keyed by alias, under a rename that lines two forms up.
#[derive(Debug, PartialEq)]
struct Fingerprint {
    nodes: Vec<String>,
    edges: Vec<String>,
    sink: Option<String>,
}

fn strip(prefix: &str) -> impl Fn(&str) -> String + '_ {
    move |alias| alias.strip_prefix(prefix).unwrap_or(alias).to_string()
}

fn fingerprint(graph: &IRGraph, rename: impl Fn(&str) -> String) -> Fingerprint {
    let name = |id: NodeId| rename(&graph.get_node(id).expect("endpoint must exist").alias);
    let mut nodes: Vec<String> = graph
        .nodes()
        .map(|n| {
            format!(
                "{} {}::{} {:?}",
                name(n.id),
                n.namespace,
                n.node_type,
                n.params
            )
        })
        .collect();
    let mut edges: Vec<String> = graph
        .edges()
        .iter()
        .map(|e| {
            format!(
                "{}{:?} -> {}{:?}",
                name(e.source),
                e.source_port,
                name(e.sink),
                e.sink_port
            )
        })
        .collect();
    nodes.sort();
    edges.sort();
    Fingerprint {
        nodes,
        edges,
        sink: graph.sink.map(name),
    }
}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

fn render_value(val: &Value) -> String {
    let list = |xs: &[Value]| xs.iter().map(render_value).collect::<Vec<_>>().join(", ");
    match val {
        Value::Null => "null".to_string(),
        Value::U32(x) => x.to_string(),
        Value::I32(x) => x.to_string(),
        Value::F32(x) => format!("{x:.4}"),
        Value::Bool(x) => x.to_string(),
        Value::Ident(x) => x.clone(),
        Value::String(x) => format!("{x:?}"),
        // The parser keeps the sigil, so it is already there.
        Value::Template(x) => x.clone(),
        Value::Array(xs) => format!("[{}]", list(xs)),
        Value::Object(o) => format!("{{ {} }}", render_params(o)),
    }
}

fn render_params(params: &Object) -> String {
    params
        .iter()
        .map(|(k, v)| format!("{k}: {}", render_value(v)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_selector(sel: &NodeSelector) -> String {
    match sel {
        NodeSelector::Single => String::new(),
        NodeSelector::All => "(*)".to_string(),
        NodeSelector::Index(i) => format!("({i})"),
        NodeSelector::Range(a, b) => format!("({a}..{b})"),
    }
}

fn render_port(port: &Port) -> String {
    match port {
        Port::None => String::new(),
        Port::Named(n) => format!(".{n}"),
        Port::Index(i) => format!("[{i}]"),
        Port::Slice(a, b) => format!("[{a}..{b}]"),
        Port::Stride { start, end, stride } => format!("[{start}:{end}:{stride}]"),
    }
}

fn render_endpoint(endpoint: &Endpoint) -> String {
    format!(
        "{}{}{}",
        endpoint.node,
        render_selector(&endpoint.node_selector),
        render_port(&endpoint.port)
    )
}

fn render_decl(decl: &NodeDeclaration) -> String {
    let alias = decl
        .alias
        .as_ref()
        .map_or(String::new(), |a| format!(": {a}"));
    let count = match decl.count {
        1 => String::new(),
        n => format!(" * {n}"),
    };
    let params = decl.params.as_ref().map_or(String::new(), render_params);
    format!("{}{alias}{count} {{ {params} }}", decl.node_type)
}

fn render_scope(scope: &DeclarationScope, indent: &str) -> String {
    let decls: String = scope
        .declarations
        .iter()
        .map(|d| format!("{indent}    {},\n", render_decl(d)))
        .collect();
    format!("{indent}{} {{\n{decls}{indent}}}\n", scope.namespace)
}

fn render_conns(conns: &[Connection], indent: &str) -> String {
    conns
        .iter()
        .map(|c| {
            format!(
                "{indent}{} >> {}\n",
                render_endpoint(&c.source),
                render_endpoint(&c.sink)
            )
        })
        .collect()
}

fn render_macro(mac: &AstMacro) -> String {
    let keyword = match mac.kind {
        MacroKind::Patch => "patch",
        MacroKind::Kernel => "kernel",
    };
    let defaults = mac.default_params.as_ref().map_or(String::new(), |p| {
        p.iter()
            .map(|(k, v)| format!("{k} = {}", render_value(v)))
            .collect::<Vec<_>>()
            .join(", ")
    });
    let mut src = format!("{keyword} {}({defaults}) {{\n", mac.name);
    if !mac.virtual_ports_in.is_empty() {
        let names: Vec<&str> = mac.virtual_ports_in.iter().map(String::as_str).collect();
        src.push_str(&format!("    in {}\n", names.join(" ")));
    }
    for scope in &mac.declarations {
        src.push_str(&render_scope(scope, "    "));
    }
    src.push_str(&render_conns(&mac.connections, "    "));
    src.push_str(&format!("    {{ {} }}\n}}\n", mac.sink));
    src
}

/// Print an [`Ast`] as DSL source.
fn render(ast: &Ast) -> String {
    let mut src = String::new();
    if let Some(source) = &ast.source {
        src.push_str(&format!("{{ {source} }}\n"));
    }
    for mac in &ast.macros {
        src.push_str(&render_macro(mac));
    }
    for scope in &ast.declarations {
        src.push_str(&render_scope(scope, ""));
    }
    src.push_str(&render_conns(&ast.connections, ""));
    src.push_str(&format!("{{ {} }}\n", ast.sink));
    src
}

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

fn selector() -> impl Strategy<Value = NodeSelector> {
    prop_oneof![
        Just(NodeSelector::Single),
        Just(NodeSelector::All),
        (0..8usize).prop_map(NodeSelector::Index),
        (0..8usize, 0..8usize).prop_map(|(a, b)| NodeSelector::Range(a, b)),
    ]
}

fn port() -> impl Strategy<Value = Port> {
    // Slice and Stride couple port width to instance count; target 5 adds them.
    prop_oneof![
        Just(Port::None),
        (0..PORT_NAMES.len()).prop_map(|i| Port::Named(PORT_NAMES[i].to_string())),
        (0..4usize).prop_map(Port::Index),
    ]
}

/// Keys are shared between leaves and patches, so some bind a default and some don't.
fn params() -> impl Strategy<Value = Object> {
    prop::collection::btree_map(
        (0..3usize).prop_map(|i| format!("d{i}")),
        prop_oneof![
            (-1000.0f32..1000.0).prop_map(Value::F32),
            (0..3usize).prop_map(|i| Value::Template(format!("$d{i}"))),
        ],
        0..3,
    )
}

fn decl_spec() -> impl Strategy<Value = DeclSpec> {
    (any::<bool>(), any::<bool>(), 0..4usize, 1u32..=3, params())
}

fn vport_spec() -> impl Strategy<Value = VPortSpec> {
    (
        any::<bool>(),
        prop::collection::vec(0..8usize, 0..3),
        any::<bool>(),
    )
}

fn multiplicity() -> impl Strategy<Value = Multiplicity> {
    (
        prop::collection::vec(macro_spec(), 0..3),
        any::<bool>(),
        2u32..=4,
        params(),
        prop::option::of(selector()),
        prop::option::of(selector()),
    )
        .prop_map(|(macros, use_patch, n, params, upstream, downstream)| {
            build_multiplicity(macros, use_patch, n, params, upstream, downstream)
        })
}

fn transparency() -> impl Strategy<Value = Transparency> {
    (
        prop::collection::vec(macro_spec(), 0..4),
        prop::collection::vec(decl_spec(), 1..5),
        prop::collection::vec(wire_spec(), 0..5),
        prop::collection::vec(vport_spec(), 0..4),
        0..8usize,
        any::<bool>(),
        prop::collection::vec((any::<bool>(), any::<bool>()), 0..6),
    )
        .prop_map(|(macros, decls, wires, vports, sink, downstream, picks)| {
            build_transparency(macros, decls, wires, vports, sink, downstream, picks)
        })
}

fn wire_spec() -> impl Strategy<Value = WireSpec> {
    (0..8usize, 0..8usize, selector(), port(), selector(), port())
}

fn macro_spec() -> impl Strategy<Value = MacroSpec> {
    (
        0..3usize,
        0..3usize,
        prop::collection::vec(decl_spec(), 1..4),
        prop::collection::vec(wire_spec(), 0..4),
        prop::collection::vec(0..8usize, 0..3),
        0..8usize,
    )
}

fn ast() -> impl Strategy<Value = Ast> {
    (
        prop::collection::vec(macro_spec(), 0..3),
        prop::collection::vec(decl_spec(), 2..6),
        prop::collection::vec(wire_spec(), 1..8),
        0..8usize,
    )
        .prop_map(|(macro_specs, decl_specs, wire_specs, sink)| {
            // Patch `i` may only instantiate patches `< i`, so the registry stays acyclic.
            let mut macros: Vec<AstMacro> = Vec::new();
            for (i, spec) in macro_specs.into_iter().enumerate() {
                let mac = build_macro(i, spec, &macros);
                macros.push(mac);
            }
            let decls = build_decls(decl_specs, &macros, "a");
            let connections = build_conns(wire_specs, &decls);
            Ast {
                sink: alias_of(&decls[sink % decls.len()]).to_string(),
                declarations: vec![DeclarationScope {
                    namespace: NAMESPACE.to_string(),
                    declarations: decls,
                }],
                connections,
                macros,
                source: None,
            }
        })
}

// ---------------------------------------------------------------------------
// Properties
// ---------------------------------------------------------------------------

proptest! {
    /// P1: a well-formed program lowers to a graph; the pipeline never unwinds.
    #[test]
    fn pipeline_lowers_without_panicking(ast in ast()) {
        let src = render(&ast);
        let graph = Pipeline::default().run_from_ast(ast).expect("unique aliases must lower");

        prop_assert!(graph.node_count() > 0, "empty graph from:\n{}", src);
        prop_assert!(!graph.has_unresolved_macros(), "macros left in:\n{}", src);
        prop_assert!(graph.sink.is_some(), "sink lost by:\n{}", src);
        // Acyclic by construction, so this must not hit the cycle assert.
        prop_assert_eq!(graph.topological_sort().len(), graph.node_count());
    }

    /// P2: printing an `Ast` and parsing it back lowers to the same graph.
    #[test]
    fn rendered_source_lowers_identically(ast in ast()) {
        let src = render(&ast);
        let direct = Pipeline::default().run_from_ast(ast).expect("unique aliases must lower");

        let reparsed = legato_parser(&src).expect("rendered source must parse");
        let via_text = Pipeline::default().run_from_ast(reparsed).expect("reparsed must lower");

        prop_assert_eq!(via_text.node_count(), direct.node_count(), "in:\n{}", src);
        prop_assert_eq!(via_text.edge_count(), direct.edge_count(), "in:\n{}", src);
    }

    /// P3: the alias index is a bijection, so expansion never shadows a node.
    #[test]
    fn alias_index_is_bijective(ast in ast()) {
        let src = render(&ast);
        let graph = Pipeline::default().run_from_ast(ast).expect("unique aliases must lower");

        let mut aliases: Vec<&str> = graph.nodes().map(|n| n.alias.as_str()).collect();
        aliases.sort_unstable();
        aliases.dedup();
        prop_assert_eq!(aliases.len(), graph.node_count(), "shadowed alias in:\n{}", src);

        for node in graph.nodes() {
            prop_assert_eq!(
                graph.resolve_alias(&node.alias),
                Some(node.id),
                "alias '{}' does not resolve to its own node in:\n{}",
                node.alias,
                src
            );
        }
    }

    /// P4: a repeated alias is rejected rather than silently shadowing a node.
    #[test]
    fn duplicate_alias_is_rejected(mut ast in ast(), pick in 0..8usize) {
        let decls = &mut ast.declarations[0].declarations;
        let victim = pick % decls.len();
        let clash = alias_of(&decls[(victim + 1) % decls.len()]).to_string();
        decls[victim].alias = Some(clash.clone());

        let src = render(&ast);
        let err = Pipeline::default().run_from_ast(ast).unwrap_err();
        prop_assert!(
            matches!(err, ValidationError::DuplicateAlias(_)),
            "expected DuplicateAlias for '{}', got {:?} in:\n{}",
            clash,
            err,
            src
        );
    }

    /// P5: a patch is transparent — instantiating it equals inlining its body.
    #[test]
    fn patch_expands_to_its_inlined_body(case in transparency()) {
        let (patch_src, inline_src) = (render(&case.patched), render(&case.inlined));
        let patched = Pipeline::default()
            .run_from_ast(case.patched)
            .expect("patched form must lower");
        let inlined = Pipeline::default()
            .run_from_ast(case.inlined)
            .expect("inlined form must lower");

        prop_assert_eq!(
            fingerprint(&patched, strip(&format!("{INSTANCE}."))),
            fingerprint(&inlined, strip("")),
            "patched:\n{}\ninlined:\n{}",
            patch_src,
            inline_src
        );
    }

    /// P6: `x * n` equals declaring `x` n times, for leaves and for patches.
    #[test]
    fn spawning_n_equals_declaring_n(case in multiplicity()) {
        let (spawn_src, declare_src) = (render(&case.spawned), render(&case.declared));
        let spawned = Pipeline::default()
            .run_from_ast(case.spawned)
            .expect("spawned form must lower");
        let declared = Pipeline::default()
            .run_from_ast(case.declared)
            .expect("declared form must lower");

        // Instance `i` of the spawn is the `i`th separate declaration.
        let unspawn = |alias: &str| match alias.strip_prefix(&format!("{SUBJECT}.")) {
            Some(rest) => format!("{SUBJECT}{rest}"),
            None => alias.to_string(),
        };

        prop_assert_eq!(
            fingerprint(&spawned, unspawn),
            fingerprint(&declared, strip("")),
            "spawned:\n{}\ndeclared:\n{}",
            spawn_src,
            declare_src
        );
    }
}

/// Writing the same node type twice without aliases is the natural way to hit this.
#[test]
fn repeated_node_type_without_alias_is_rejected() {
    let ast = legato_parser("audio { sine { }, sine { } }\n{ sine }").expect("parses");
    let err = Pipeline::default().run_from_ast(ast).unwrap_err();
    assert!(matches!(err, ValidationError::DuplicateAlias(_)), "{err:?}");
}

/// A sink-side selector must mean the same thing for a patch as for a leaf.
#[test]
fn sink_selector_picks_patch_instances() {
    let edges = |src: &str| {
        let graph = Pipeline::default()
            .run_from_ast(legato_parser(src).expect("parses"))
            .expect("lowers");
        graph.edge_count()
    };
    let leaf = "audio { leaf: src { }, leaf: a * 3 { } }\nsrc >> a(0)\n{ a }";
    let patch = "patch p() { audio { leaf: b { } } { b } }\n\
                 audio { leaf: src { }, p: a * 3 { } }\nsrc >> a(0)\n{ a }";
    assert_eq!(edges(leaf), 1);
    assert_eq!(
        edges(patch),
        1,
        "a patch instance ignored its sink selector"
    );
}

/// Instance arity is exact: a patch pairs by selected instance, like a leaf.
#[test]
fn patch_instance_arity_matches_leaf() {
    const PATCH: &str = "patch p() { audio { leaf: b { } } { b } }\n";

    let edges = |decls: &str, conn: &str| -> Result<usize, ValidationError> {
        let src = format!("{PATCH}audio {{ {decls} }}\n{conn}\n{{ snk }}");
        let ast = legato_parser(&src).expect("parses");
        Pipeline::default()
            .run_from_ast(ast)
            .map(|g| g.edge_count())
    };

    // Both directions matter: a patch source splits its outgoing edges, a patch
    // sink resolves its incoming ones, and each has its own arity check.
    for (src_ty, snk_ty) in [("leaf", "leaf"), ("leaf", "p"), ("p", "leaf"), ("p", "p")] {
        let at = |s: u32, k: u32| format!("{src_ty}: src * {s} {{ }}, {snk_ty}: snk * {k} {{ }}");
        let case = format!("{src_ty}->{snk_ty}");

        // 2:4 has no exact pairing and neither side is single.
        assert!(
            matches!(
                edges(&at(4, 4), "src(0..2) >> snk(*)"),
                Err(ValidationError::SelectionArity(_))
            ),
            "{case} 2:4 should be a selection-arity error"
        );
        // 2:2 zips the selected instances, however they were declared.
        assert_eq!(
            edges(&at(4, 4), "src(0..2) >> snk(0..2)"),
            Ok(2),
            "{case} 2:2"
        );
        assert_eq!(edges(&at(4, 2), "src(0..2) >> snk(*)"), Ok(2), "{case} 4/2");
        // One source still broadcasts to many sinks.
        assert_eq!(edges(&at(4, 4), "src(0) >> snk(*)"), Ok(4), "{case} 1:4");
    }
}

/// Expansion order must not depend on which nodes were removed before it.
#[test]
fn identical_patch_instances_expand_identically() {
    let src = r#"
        patch p0() { audio { leaf: b0 { } } { b0 } }
        patch p1() {
            audio { leaf: b0 { }, p0: b1 * 2 { }, p0: b2 * 2 { } }
            b1(0..1) >> b2
            { b0 }
        }
        audio { p1: a0 { }, p1: a1 { } }
        { a1 }
    "#;
    let graph = Pipeline::default()
        .run_from_ast(legato_parser(src).expect("parses"))
        .expect("lowers");

    let wiring = |instance: &str| {
        let mut edges: Vec<String> = graph
            .edges()
            .iter()
            .map(|e| {
                let named = |id| graph.get_node(id).expect("endpoint exists").alias.clone();
                (named(e.source), named(e.sink))
            })
            .filter(|(src, _)| src.starts_with(instance))
            .map(|(src, snk)| format!("{src} -> {snk}"))
            .map(|s| s.replace(instance, "_"))
            .collect();
        edges.sort();
        edges
    };
    assert_eq!(wiring("a0"), wiring("a1"));
}

#[test]
fn duplicate_alias_inside_patch_is_rejected() {
    let src = r#"
        patch p() {
            audio { leaf: b { }, leaf: b { } }
            { b }
        }
        audio { p: a { } }
        { a }
    "#;
    let ast = legato_parser(src).expect("parses");
    let err = Pipeline::default().run_from_ast(ast).unwrap_err();
    assert!(matches!(err, ValidationError::DuplicateAlias(_)), "{err:?}");
}

/// Resolved edges keyed by endpoint alias and port, order-independent.
fn edge_set(graph: &IRGraph) -> BTreeSet<String> {
    let alias = |id| graph.get_node(id).expect("endpoint must exist").alias.clone();
    graph
        .edges()
        .iter()
        .map(|e| {
            format!(
                "{}{:?} -> {}{:?}",
                alias(e.source),
                e.source_port,
                alias(e.sink),
                e.sink_port
            )
        })
        .collect()
}

fn lower_src(src: &str) -> IRGraph {
    Pipeline::default()
        .run_from_ast(legato_parser(src).expect("source must parse"))
        .expect("source must lower")
}

proptest! {
    /// Flatten-distributes (single dimension, `sp = 1`): `src(*) >> mixer[0..n]`
    /// zips instance `i` onto port `i`, which must be identical to writing the n
    /// explicit `src(i) >> mixer[i]` wires. This is the invariant the poly-voice
    /// clipping bug violated — it fanned each instance across *every* port (an
    /// `n:n` cross-product plus a summing mixer) instead of a one-to-one zip.
    #[test]
    fn all_selector_slice_equals_explicit_wires(n in 2u32..=8) {
        let decls = format!("audio {{ leaf: src * {n} {{ }}, leaf: mixer {{ }} }}");
        let sliced = format!("{decls}\nsrc(*) >> mixer[0..{n}]\n{{ mixer }}");
        let explicit: String = (0..n).map(|i| format!("src({i}) >> mixer[{i}]\n")).collect();
        let explicit = format!("{decls}\n{explicit}{{ mixer }}");

        prop_assert_eq!(
            edge_set(&lower_src(&sliced)),
            edge_set(&lower_src(&explicit)),
            "sliced:\n{}\nexplicit:\n{}",
            sliced,
            explicit
        );
    }

    /// Flatten-distributes (`sp > 1`): `src(*)[0..sp] >> mixer[0..n*sp]` lays each
    /// instance's `sp` ports onto a contiguous window, instance-major. It must
    /// equal writing every `src(i)[j] >> mixer[i*sp + j]` line by hand — a single
    /// flatten axis, no cross-product. The exact `n*sp == width` check means a
    /// mismatched width is rejected rather than silently inferred.
    #[test]
    fn flatten_slice_equals_explicit_wires(n in 2u32..=6, sp in 2u32..=4) {
        let width = n * sp;
        let decls = format!("audio {{ leaf: src * {n} {{ }}, leaf: mixer {{ }} }}");
        let sliced = format!("{decls}\nsrc(*)[0..{sp}] >> mixer[0..{width}]\n{{ mixer }}");
        let explicit: String = (0..n)
            .flat_map(|i| (0..sp).map(move |j| format!("src({i})[{j}] >> mixer[{}]\n", i * sp + j)))
            .collect();
        let explicit = format!("{decls}\n{explicit}{{ mixer }}");

        prop_assert_eq!(
            edge_set(&lower_src(&sliced)),
            edge_set(&lower_src(&explicit)),
            "sliced:\n{}\nexplicit:\n{}",
            sliced,
            explicit
        );
    }

    /// A width that is not `instances * source_ports` has no single-axis flatten
    /// and must be a selection-arity error, never a silent cross-product.
    #[test]
    fn mismatched_flatten_width_is_rejected(n in 2u32..=6, sp in 1u32..=3, slack in 1u32..=3) {
        // Any width the exact product can't hit (offset by `slack`, avoiding 0).
        let width = n * sp + slack;
        let src = format!(
            "audio {{ leaf: src * {n} {{ }}, leaf: mixer {{ }} }}\n\
             src(*)[0..{sp}] >> mixer[0..{width}]\n{{ mixer }}"
        );
        let err = Pipeline::default()
            .run_from_ast(legato_parser(&src).expect("parses"))
            .unwrap_err();
        prop_assert!(
            matches!(err, ValidationError::SelectionArity(_)),
            "expected SelectionArity for {n}x{sp} into width {width}, got {:?}\n{}",
            err,
            src
        );
    }
}
