use crate::dsl::{expand::MacroExpansionPass, ir::*, lower::ast_to_graph, spawn::SpawnKNodesPass};

/// A single, named transformation of an [`IRGraph`].
pub trait GraphPass {
    fn name(&self) -> &'static str;
    fn run(&self, graph: IRGraph) -> IRGraph;
}

/// An ordered sequence of [`GraphPass`]es applied to an [`IRGraph`].
pub struct Pipeline {
    passes: Vec<Box<dyn GraphPass>>,
}

impl Pipeline {
    pub fn new() -> Self {
        Self { passes: vec![] }
    }

    /// Append a pass to the end of the pipeline.
    pub fn add_pass<P: GraphPass + 'static>(mut self, pass: P) -> Self {
        self.passes.push(Box::new(pass));
        self
    }

    /// Translate `ast` to a literal [`IRGraph`] (see [`ast_to_graph`]), then
    /// run all passes in order.
    pub fn run_from_ast(self, ast: Ast) -> IRGraph {
        let initial = ast_to_graph(ast);
        self.run(initial)
    }

    /// Run all passes on an already-constructed graph.
    pub fn run(self, graph: IRGraph) -> IRGraph {
        self.passes.into_iter().fold(graph, |g, pass| pass.run(g))
    }
}

impl Default for Pipeline {
    /// The default pipeline. This will eventually handle sample rates, spawning nodes N times, etc.
    fn default() -> Self {
        Self::new()
            .add_pass(MacroExpansionPass::default())
            .add_pass(SpawnKNodesPass::default())
    }
}
