//! Property tests for the DSL pipeline: lower -> expand -> spawn -> resolve.
//!
//! Generated programs are real [`Ast`]s. Values that need no context
//! (`NodeSelector`, `Port`, `Value`) are drawn directly; the composites are built
//! afterwards, because `Ast` addresses nodes by name while a generator addresses
//! them by position. Aliases are derived from position, so they are unique.

use legato::dsl::ir::*;
use legato::dsl::parse::legato_parser;
use legato::dsl::pipeline::Pipeline;
use proptest::prelude::*;

const NAMESPACE: &str = "audio";
const LEAF_TYPE: &str = "leaf";
/// Endpoint port names; some hit a patch's virtual ports, some deliberately don't.
const PORT_NAMES: [&str; 3] = ["v0", "v1", "sig"];

/// `(instantiate a patch, which one, instance count, params)`.
type DeclSpec = (bool, usize, u32, Object);
/// A connection with positional endpoints, named when the declarations exist.
type WireSpec = (usize, usize, NodeSelector, Port, NodeSelector, Port);
/// `(defaults, virtual ports, body, wiring, virtual-port targets, sink)`.
type MacroSpec = (usize, usize, Vec<DeclSpec>, Vec<WireSpec>, Vec<usize>, usize);

// ---------------------------------------------------------------------------
// Build
// ---------------------------------------------------------------------------

/// Mirrors the fallback in `lower::ast_to_graph`.
fn alias_of(decl: &NodeDeclaration) -> &str {
    decl.alias.as_deref().unwrap_or(&decl.node_type)
}

fn is_patch(decl: &NodeDeclaration, macros: &[AstMacro]) -> bool {
    macros.iter().any(|m| m.name == decl.node_type)
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
    specs
        .into_iter()
        .enumerate()
        .map(|(i, (want_patch, pick, count, params))| NodeDeclaration {
            node_type: match want_patch && !macros.is_empty() {
                true => macros[pick % macros.len()].name.clone(),
                false => LEAF_TYPE.to_string(),
            },
            // Always named, since `None` aliases collide by node type — see target 2.
            alias: Some(format!("{prefix}{i}")),
            params: Some(params),
            count,
        })
        .collect()
}

fn build_conns(
    specs: Vec<WireSpec>,
    decls: &[NodeDeclaration],
    macros: &[AstMacro],
) -> Vec<Connection> {
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

            // Expansion matches raw counts, not selectors, on the way into a patch.
            if is_patch(snk, macros) && !broadcastable(src.count as usize, snk.count as usize) {
                return None;
            }
            clamp(&mut src_sel, src.count);
            clamp(&mut snk_sel, snk.count);

            // Selector arity only binds leaf to leaf; a patch always fans into
            // single instances, so its own selectors cannot mismatch.
            let bound = !is_patch(src, macros) && !is_patch(snk, macros);
            if bound
                && !broadcastable(
                    src_sel.selected_count(src.count as usize),
                    snk_sel.selected_count(snk.count as usize),
                )
            {
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
        connections.push(Connection {
            source: Endpoint {
                node: format!("v{}", i % vports),
                node_selector: NodeSelector::Single,
                port: Port::None,
            },
            sink: Endpoint {
                node: alias_of(&body[target]).to_string(),
                node_selector: NodeSelector::Single,
                port: Port::None,
            },
        });
    }
    connections.extend(build_conns(wire_specs, &body, earlier));

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
    let params = decl.params.as_ref().map_or(String::new(), |p| render_params(p));
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
    (any::<bool>(), 0..4usize, 1u32..=3, params())
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
            let connections = build_conns(wire_specs, &decls, &macros);
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
        let graph = Pipeline::default().run_from_ast(ast);

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
        let direct = Pipeline::default().run_from_ast(ast);

        let reparsed = legato_parser(&src).expect("rendered source must parse");
        let via_text = Pipeline::default().run_from_ast(reparsed);

        prop_assert_eq!(via_text.node_count(), direct.node_count(), "in:\n{}", src);
        prop_assert_eq!(via_text.edge_count(), direct.edge_count(), "in:\n{}", src);
    }
}
