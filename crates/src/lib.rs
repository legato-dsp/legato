use std::collections::HashMap;


#[cfg(feature = "core")]
pub use legato_core as core;

use legato_dsl::{ast::build_ast, ir::{build_runtime_from_ast, node_spec::NodeSpec, registry::AudioRegistry}, parse::parse_legato_file};

#[cfg(feature = "dsl")]
pub use legato_dsl as dsl;

#[cfg(feature="default")]
use legato_core::runtime::context::Config;
use legato_core::{nodes::ports::Ports, runtime::runtime::{Runtime, RuntimeBackend}};

#[cfg(feature="default")]
pub struct Legato {
    registries: HashMap<String, AudioRegistry>,
    legato_file: Option<String>,
    config: Option<Config>,

}

impl Legato {
    /// Build a new Legato graph with the default registry
    pub fn new() -> Self {
        let default_registry = AudioRegistry::default();

        let mut registries = HashMap::new();
        registries.insert("audio".into(), default_registry);

        let user_registry = AudioRegistry::new();
        registries.insert("user".into(), user_registry);

        Self {
            registries,
            legato_file: None,
            config: None
        }
    }
    /// Add a subgraph to the "user" node namespace.
    /// 
    /// Note: Subgraphs do not have params. If you need something
    /// more finetuned, define a subgraph as a node.
    pub fn subgraph(&mut self, name: String, subgraph: Box<Runtime>, ports: Ports) {
        todo!()
    }
    /// Add a node into the "user" node namespace.
    /// 
    /// You can do this with a node spec, which tells Legato
    /// how to map it and call a builder
    pub fn define(&mut self, name: String, node_spec: NodeSpec){
        let user_name_space = self.registries.get_mut("user".into()).expect("Could not get user namespace!");
        user_name_space.declare_node(name, node_spec);
    }
    /// Add a node namespace with a name and a registry
    pub fn registry(&mut self, name: String, registry: AudioRegistry){
        self.registries.insert(name, registry).expect("Could not insert registry");
    }
    /// Add the config for your project
    pub fn config(&mut self, config: Config){
        self.config = Some(config);
    }
    /// Load in the final runtime graph
    pub fn load(&mut self, graph: String) {
        self.legato_file = Some(graph);
    }
    pub fn build(&self) -> (Runtime, RuntimeBackend) {
        let config = self.config.expect("Cannot build with no config. Call the config method to instantiate");
        
        let file = self.legato_file
            .as_ref()
            .expect("No legato file found.");

        let parsed = parse_legato_file(&file);    

        let ast = build_ast(parsed.unwrap()).unwrap();
        build_runtime_from_ast(ast, &self.registries, config)
    }
}