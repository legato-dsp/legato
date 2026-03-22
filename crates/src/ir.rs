use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    fmt,
    time::Duration,
};

use indexmap::IndexSet;

use crate::builder::ValidationError;

// ---------------------------------------------------------------------------
// Shared primitive types
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    U32(u32),
    I32(i32),
    F32(f32),
    Bool(bool),
    Ident(String),
    String(String),
    Array(Vec<Value>),
    Object(BTreeMap<String, Value>),
    Template(String),
}

pub type Object = BTreeMap<String, Value>;

#[derive(Debug, Clone, PartialEq)]
pub enum Port {
    Named(String),
    Index(usize),
    Slice(usize, usize),
    Stride {
        start: usize,
        end: usize,
        stride: usize,
    },
    None,
}

// ---------------------------------------------------------------------------
// AST types — produced by the parser, consumed by `ast_to_graph`
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ASTPipe {
    pub name: String,
    pub params: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct NodeDeclaration {
    pub node_type: String,
    pub alias: Option<String>,
    pub params: Option<Object>,
    pub pipes: Vec<ASTPipe>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct DeclarationScope {
    pub namespace: String,
    pub declarations: Vec<NodeDeclaration>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Endpoint {
    pub node: String,
    pub port: Port,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Connection {
    pub source: Endpoint,
    pub sink: Endpoint,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Macro {
    pub name: String,
    pub default_params: Option<Object>,
    pub virtual_ports_in: IndexSet<String>,
    pub declarations: Vec<DeclarationScope>,
    pub connections: Vec<Connection>,
    pub sink: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Ast {
    pub declarations: Vec<DeclarationScope>,
    pub connections: Vec<Connection>,
    pub macros: Vec<Macro>,
    pub sink: String,
    pub source: Option<String>,
}

// ---------------------------------------------------------------------------
// Graph IR node kinds
// ---------------------------------------------------------------------------

/// Whether an [`IRNode`] is a concrete leaf or an unexpanded macro reference.
///
/// The graph passes through two broad states:
///
/// 1. **Before [`MacroExpansionPass`]** — the graph is a literal mirror of the
///    DSL source.  Top-level macro instantiations appear as `MacroRef` nodes,
///    connected to each other with the ports written in the source.  This is
///    the most human-readable form of the graph.
///
/// 2. **After [`MacroExpansionPass`]** — every node is a `Leaf`.  Aliases are
///    now fully-qualified (`"lead.osc_inst.carrier"`), params have been
///    substituted, and virtual ports have been resolved to concrete edges.
///    The graph is ready for the builder.
///
/// Subsequent passes (sample-rate boundaries, port expansion, etc.) only
/// ever see `Leaf` nodes and operate purely on graph topology.
#[derive(Debug, Clone, PartialEq)]
pub enum IRNodeKind {
    /// A concrete, instantiable node.  The builder can call its factory
    /// function directly.
    Leaf,
    /// An unexpanded macro reference.  `node_type` names the macro.
    /// Removed by [`MacroExpansionPass`].
    MacroRef,
}

// ---------------------------------------------------------------------------
// Graph IR nodes and edges
// ---------------------------------------------------------------------------

/// Opaque, stable identifier for a node in the [`IRGraph`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(u32);

impl NodeId {
    pub fn index(self) -> u32 {
        self.0
    }
}

/// A single node in the graph IR.
#[derive(Debug, Clone)]
pub struct IRNode {
    pub id: NodeId,
    pub kind: IRNodeKind,
    /// Namespace this node was declared in (e.g. `"audio"`, `"midi"`).
    /// For `MacroRef` nodes this reflects the instantiation-site namespace;
    /// after expansion each leaf carries its own scope's namespace.
    pub namespace: String,
    /// Concrete node type or macro name.
    pub node_type: String,
    /// Alias as written in source (pre-expansion) or fully-qualified name
    /// produced by macro expansion (post-expansion).
    pub alias: String,
    pub params: Object,
    pub pipes: Vec<ASTPipe>,
}

/// A directed edge connecting an output port of one node to an input port
/// of another.
#[derive(Debug, Clone, PartialEq)]
pub struct IREdge {
    pub source: NodeId,
    pub source_port: Port,
    pub sink: NodeId,
    pub sink_port: Port,
}

// ---------------------------------------------------------------------------
// IRGraph
// ---------------------------------------------------------------------------

/// Directed graph of audio/control nodes and their connections.
///
/// This is the central data structure of the compiler pipeline.  Each
/// [`GraphPass`] consumes an `IRGraph` and returns a transformed one, making
/// every step of compilation explicit and inspectable.
///
/// ## Pipeline lifecycle
///
/// ```text
/// Ast ──ast_to_graph()──► IRGraph            (literal; may contain MacroRef nodes)
///                              │
///                    MacroExpansionPass       (expands MacroRefs → Leaf clusters)
///                              │
///                    SampleRateBoundaryPass   (future: inserts converter nodes)
///                    PortExpansionPass        (future: expands Port::Slice etc.)
///                              │
///                              ▼
///                         IRGraph             (all Leaf nodes, builder-ready)
/// ```
#[derive(Debug, Default)]
pub struct IRGraph {
    /// IndexMap preserves insertion order, giving a stable topological-sort
    /// baseline when independent nodes have no ordering constraint.
    nodes: IndexMap<NodeId, IRNode>,
    edges: Vec<IREdge>,
    alias_index: HashMap<String, NodeId>,
    next_id: u32,
    pub sink: Option<NodeId>,
    pub source: Option<NodeId>,
    /// Macro definitions carried through the pipeline.
    ///
    /// Populated by [`ast_to_graph`] and kept alive so that any pass can
    /// inspect or further expand macros.  After [`MacroExpansionPass`] this
    /// map is still present but no longer referenced by any node in the graph.
    pub macro_registry: HashMap<String, Macro>,
}

// IndexMap is not in std; re-export the dependency for the field type
use indexmap::IndexMap;

impl IRGraph {
    pub fn new() -> Self {
        Self::default()
    }

    // -----------------------------------------------------------------------
    // Mutation
    // -----------------------------------------------------------------------

    /// Insert a node and return its [`NodeId`].
    pub fn add_node(
        &mut self,
        kind: IRNodeKind,
        namespace: impl Into<String>,
        node_type: impl Into<String>,
        alias: impl Into<String>,
        params: Object,
        pipes: Vec<ASTPipe>,
    ) -> NodeId {
        let id = NodeId(self.next_id);
        self.next_id += 1;
        let alias = alias.into();
        let node = IRNode {
            id,
            kind,
            namespace: namespace.into(),
            node_type: node_type.into(),
            alias: alias.clone(),
            params,
            pipes,
        };
        self.nodes.insert(id, node);
        self.alias_index.insert(alias, id);
        id
    }

    /// Add a directed edge.
    pub fn connect(&mut self, source: NodeId, source_port: Port, sink: NodeId, sink_port: Port) {
        self.edges.push(IREdge {
            source,
            source_port,
            sink,
            sink_port,
        });
    }

    /// Splice a new node into an existing edge:
    ///
    /// ```text
    /// before:  A ──[edge]──► B
    /// after:   A ──► new ──► B
    /// ```
    ///
    /// The original source port is preserved on the A→new half; the original
    /// sink port is preserved on the new→B half.  The caller receives the
    /// `NodeId` of the inserted node.
    pub fn insert_between(
        &mut self,
        edge_index: usize,
        namespace: impl Into<String>,
        node_type: impl Into<String>,
        alias: impl Into<String>,
        params: Object,
    ) -> NodeId {
        let edge = self.edges.remove(edge_index);
        let new_id = self.add_node(
            IRNodeKind::Leaf,
            namespace,
            node_type,
            alias,
            params,
            vec![],
        );
        self.edges.push(IREdge {
            source: edge.source,
            source_port: edge.source_port,
            sink: new_id,
            sink_port: Port::None,
        });
        self.edges.push(IREdge {
            source: new_id,
            source_port: Port::None,
            sink: edge.sink,
            sink_port: edge.sink_port,
        });
        new_id
    }

    /// Remove a node and all of its incident edges.
    pub fn remove_node(&mut self, id: NodeId) {
        if let Some(node) = self.nodes.remove(&id) {
            self.alias_index.remove(&node.alias);
        }
        self.edges.retain(|e| e.source != id && e.sink != id);
    }

    // -----------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------

    pub fn nodes(&self) -> impl Iterator<Item = &IRNode> {
        self.nodes.values()
    }
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
    pub fn edges(&self) -> &[IREdge] {
        &self.edges
    }
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    pub fn get_node(&self, id: NodeId) -> Option<&IRNode> {
        self.nodes.get(&id)
    }
    pub fn get_node_mut(&mut self, id: NodeId) -> Option<&mut IRNode> {
        self.nodes.get_mut(&id)
    }

    pub fn find_node_by_alias(&self, alias: &str) -> Option<&IRNode> {
        self.alias_index
            .get(alias)
            .and_then(|id| self.nodes.get(id))
    }

    pub fn resolve_alias(&self, alias: &str) -> Option<NodeId> {
        self.alias_index.get(alias).copied()
    }

    /// All nodes whose kind is [`IRNodeKind::Leaf`].
    pub fn leaf_nodes(&self) -> impl Iterator<Item = &IRNode> {
        self.nodes.values().filter(|n| n.kind == IRNodeKind::Leaf)
    }

    /// All nodes whose kind is [`IRNodeKind::MacroRef`].
    pub fn macro_nodes(&self) -> impl Iterator<Item = &IRNode> {
        self.nodes
            .values()
            .filter(|n| n.kind == IRNodeKind::MacroRef)
    }

    /// `true` if any [`IRNodeKind::MacroRef`] nodes remain.
    /// Should be `false` after a successful [`MacroExpansionPass`].
    pub fn has_unresolved_macros(&self) -> bool {
        self.nodes.values().any(|n| n.kind == IRNodeKind::MacroRef)
    }

    pub fn outgoing_edges(&self, id: NodeId) -> impl Iterator<Item = &IREdge> {
        self.edges.iter().filter(move |e| e.source == id)
    }

    pub fn incoming_edges(&self, id: NodeId) -> impl Iterator<Item = &IREdge> {
        self.edges.iter().filter(move |e| e.sink == id)
    }

    pub fn successors(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.edges
            .iter()
            .filter(move |e| e.source == id)
            .map(|e| e.sink)
    }

    pub fn predecessors(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.edges
            .iter()
            .filter(move |e| e.sink == id)
            .map(|e| e.source)
    }

    pub fn find_edges_between(&self, src_alias: &str, snk_alias: &str) -> Vec<&IREdge> {
        let Some(&src) = self.alias_index.get(src_alias) else {
            return vec![];
        };
        let Some(&snk) = self.alias_index.get(snk_alias) else {
            return vec![];
        };
        self.edges
            .iter()
            .filter(|e| e.source == src && e.sink == snk)
            .collect()
    }

    pub fn find_edges_from(&self, src_alias: &str) -> Vec<&IREdge> {
        let Some(&src) = self.alias_index.get(src_alias) else {
            return vec![];
        };
        self.edges.iter().filter(|e| e.source == src).collect()
    }

    pub fn find_edges_to(&self, snk_alias: &str) -> Vec<&IREdge> {
        let Some(&snk) = self.alias_index.get(snk_alias) else {
            return vec![];
        };
        self.edges.iter().filter(|e| e.sink == snk).collect()
    }

    // -----------------------------------------------------------------------
    // Graph algorithms
    // -----------------------------------------------------------------------

    /// Topological sort (Kahn's algorithm).
    ///
    /// Returns node IDs in producer-before-consumer order.  Independent nodes
    /// are yielded in insertion order (stable).  Panics on a cycle.
    pub fn topological_sort(&self) -> Vec<NodeId> {
        let mut in_degree: HashMap<NodeId, usize> = self.nodes.keys().map(|&k| (k, 0)).collect();

        for edge in &self.edges {
            *in_degree.entry(edge.sink).or_insert(0) += 1;
        }

        let mut queue: VecDeque<NodeId> = in_degree
            .iter()
            .filter(|(_, d)| **d == 0)
            .map(|(&k, _)| k)
            .collect();
        queue.make_contiguous().sort();

        let mut sorted = Vec::with_capacity(self.nodes.len());
        while let Some(id) = queue.pop_front() {
            sorted.push(id);
            let mut next: Vec<NodeId> = self
                .edges
                .iter()
                .filter(|e| e.source == id)
                .filter_map(|e| {
                    let deg = in_degree.get_mut(&e.sink)?;
                    *deg -= 1;
                    (*deg == 0).then_some(e.sink)
                })
                .collect();
            next.sort();
            queue.extend(next);
        }

        assert_eq!(
            sorted.len(),
            self.nodes.len(),
            "IRGraph contains a cycle — topological sort is undefined"
        );
        sorted
    }
}

impl fmt::Display for IRGraph {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Build reverse map once: NodeId -> alias
        let id_to_alias: HashMap<NodeId, &str> = self
            .alias_index
            .iter()
            .map(|(alias, &id)| (id, alias.as_str()))
            .collect();

        writeln!(f, "nodes:")?;
        for node in self
            .topological_sort()
            .into_iter()
            .map(|x| self.get_node(x).unwrap())
        {
            writeln!(
                f,
                "  [{:?}] {} ({}::{})",
                node.id,
                id_to_alias.get(&node.id).unwrap_or(&"?"),
                node.namespace,
                node.node_type
            )?;
        }

        writeln!(f, "edges:")?;
        for edge in &self.edges {
            let src = id_to_alias.get(&edge.source).unwrap_or(&"?");
            let snk = id_to_alias.get(&edge.sink).unwrap_or(&"?");
            writeln!(
                f,
                "  {} {:?} -> {} {:?}",
                src, edge.source_port, snk, edge.sink_port
            )?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// DSLParams (unchanged)
// ---------------------------------------------------------------------------

pub struct DSLParams<'a>(pub &'a Object);

impl<'a> DSLParams<'a> {
    pub fn new(obj: &'a Object) -> Self {
        Self(obj)
    }

    pub fn get_f32(&self, key: &str) -> Option<f32> {
        match self.0.get(key) {
            Some(Value::F32(x)) => Some(*x),
            Some(Value::I32(x)) => Some(*x as f32),
            Some(Value::U32(x)) => Some(*x as f32),
            Some(x) => panic!("Expected F32 param, found {:?}", x),
            _ => None,
        }
    }

    // TODO: More units
    pub fn get_duration_ms(&self, key: &str) -> Option<Duration> {
        match self.0.get(key) {
            Some(Value::F32(ms)) => Some(Duration::from_secs_f32(ms / 1000.0)),
            Some(Value::I32(ms)) => Some(Duration::from_millis(*ms as u64)),
            Some(Value::U32(ms)) => Some(Duration::from_millis(*ms as u64)),
            Some(x) => panic!("Expected F32 or I32 param for ms, found {:?}", x),
            _ => None,
        }
    }

    pub fn get_u32(&self, key: &str) -> Option<u32> {
        match self.0.get(key) {
            Some(Value::U32(s)) => Some(*s),
            Some(x) => panic!("Expected U32 param, found {:?}", x),
            _ => None,
        }
    }

    pub fn get_usize(&self, key: &str) -> Option<usize> {
        self.get_u32(key).map(|i| i as usize)
    }

    pub fn get_str(&self, key: &str) -> Option<String> {
        match self.0.get(key) {
            Some(Value::String(s)) => Some(s.clone()),
            Some(Value::Ident(i)) => Some(i.clone()),
            Some(x) => panic!("Expected str param, found {:?}", x),
            _ => None,
        }
    }

    pub fn get_bool(&self, key: &str) -> Option<bool> {
        match self.0.get(key) {
            Some(Value::Bool(b)) => Some(*b),
            Some(x) => panic!("Expected bool param, found {:?}", x),
            _ => None,
        }
    }

    pub fn get_object(&self, key: &str) -> Option<Object> {
        match self.0.get(key) {
            Some(Value::Object(o)) => Some(o.clone()),
            Some(x) => panic!("Expected object param, found {:?}", x),
            _ => None,
        }
    }

    pub fn get_array(&self, key: &str) -> Option<Vec<Value>> {
        match self.0.get(key) {
            Some(Value::Array(v)) => Some(v.clone()),
            Some(x) => panic!("Expected array param, found {:?}", x),
            _ => None,
        }
    }

    pub fn get_array_f32(&self, key: &str) -> Option<Vec<f32>> {
        let arr = match self.0.get(key) {
            Some(Value::Array(v)) => Some(v.clone()),
            Some(x) => panic!("Expected array param, found {:?}", x),
            _ => None,
        }?;
        Some(
            arr.into_iter()
                .map(|x| match x {
                    Value::F32(x) => x,
                    Value::I32(x) => x as f32,
                    Value::U32(x) => x as f32,
                    _ => panic!("Unexpected value in f32 array {:?}", x),
                })
                .collect(),
        )
    }

    pub fn get_array_duration_ms(&self, key: &str) -> Option<Vec<Duration>> {
        let arr = match self.0.get(key) {
            Some(Value::Array(v)) => v.clone(),
            Some(x) => panic!("Expected array param, found {:?}", x),
            _ => return None,
        };
        Some(
            arr.into_iter()
                .map(|x| match x {
                    Value::F32(x) => Duration::from_secs_f32(x / 1000.0),
                    Value::I32(x) => Duration::from_millis(x as u64),
                    Value::U32(x) => Duration::from_millis(x as u64),
                    _ => panic!("Unexpected value in duration array {:?}", x),
                })
                .collect(),
        )
    }

    pub fn validate(&self, allowed: &BTreeSet<String>) -> Result<(), ValidationError> {
        for k in self.0.keys() {
            if !allowed.contains(k) {
                return Err(ValidationError::InvalidParameter(format!(
                    "Could not find parameter with name {}",
                    k
                )));
            }
        }
        Ok(())
    }

    pub fn required(&self, required: &BTreeSet<String>) -> Result<(), ValidationError> {
        for k in required {
            if !self.0.contains_key(k) {
                return Err(ValidationError::MissingRequiredParameter(format!(
                    "Missing required parameter {}",
                    k,
                )));
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Value conversions
// ---------------------------------------------------------------------------

impl From<f32> for Value {
    fn from(v: f32) -> Self {
        Value::F32(v)
    }
}
impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Value::I32(v)
    }
}
impl From<u32> for Value {
    fn from(v: u32) -> Self {
        Value::U32(v)
    }
}
impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}
impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::String(v.to_string())
    }
}
impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::String(v)
    }
}
impl From<BTreeMap<String, Value>> for Value {
    fn from(v: BTreeMap<String, Value>) -> Self {
        Value::Object(v)
    }
}
impl<T: Into<Value>> From<Vec<T>> for Value {
    fn from(v: Vec<T>) -> Self {
        Value::Array(v.into_iter().map(|x| x.into()).collect())
    }
}

pub struct Template(pub String);
impl From<Template> for Value {
    fn from(t: Template) -> Self {
        Value::Template(t.0)
    }
}
impl<'a> From<&'a Object> for DSLParams<'a> {
    fn from(v: &'a Object) -> Self {
        DSLParams(v)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum BuildAstError {
    ConstructionError(String),
}
