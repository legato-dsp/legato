use std::collections::HashMap;

use crate::{
    ValidationError,
    ast::Value,
    builder::{SelectionKind, SelectionView},
    node::LegatoNode,
    nodes::audio::oversample::oversample_by_two_factory,
};

pub struct PipeRegistry {
    data: HashMap<String, Box<dyn Pipe>>,
}

impl PipeRegistry {
    pub fn new(data: HashMap<String, Box<dyn Pipe>>) -> Self {
        Self { data }
    }

    pub fn insert(&mut self, name: String, pipe: Box<dyn Pipe>) {
        self.data.insert(name, pipe);
    }

    pub fn get(&self, name: &str) -> Result<&Box<dyn Pipe>, ValidationError> {
        self.data
            .get(name)
            .ok_or(ValidationError::PipeNotFound(format!(
                "Could not find pipe {}",
                name
            )))
    }
}

impl Default for PipeRegistry {
    fn default() -> Self {
        let mut data: HashMap<String, Box<dyn Pipe>> = HashMap::new();
        data.insert(String::from("replicate"), Box::new(Replicate {}));
        data.insert(String::from("oversample2X"), Box::new(Oversample2X {}));

        Self { data }
    }
}

/// Pipes are functions that can transform a node or multiple nodes.
///
/// It's important to know that pipes must be ran before any connections are formed,
/// as this would blow up the complexity.
///
/// For example, imagine you want to create a series of allpass filters,
/// with varying delay times.
///
/// Rather than instantiating that node N times,
/// you could use a replicate pipe to make N of them, then apply some transformation
/// on the parameters of all of those allpasses.
///
/// You could also say make a visualization node, that pipes any node audio you have to some
/// window screen somewhere. Your pipe would use the Selection api to take all of the
/// nodes in your selection, and then replace them with some node like so:
///
/// Visualizer {
///     node: Box<dyn DynNode>
/// }
///
/// Pipes are designed to apply lightweight transformations. If you need something more
/// powerful that is only used a few times, you may want to design a node or subgraph
/// instead.
pub trait Pipe {
    fn pipe(&self, view: &mut SelectionView, props: Option<Value>);
}

// A collection of a few default pipes

/// Basic pipe to clone a node N times.
struct Replicate;

impl Pipe for Replicate {
    fn pipe(&self, view: &mut SelectionView, props: Option<Value>) {
        let selection = view.selection();

        match selection {
            SelectionKind::Single(key) => {
                let val = props.unwrap_or(Value::U32(2));

                match val {
                    Value::U32(n) => {
                        let node = view
                            .get_node(key)
                            .expect("Could not find not key in Pipe!")
                            .clone();

                        for _ in 0..n {
                            let cloned = node.clone();
                            view.insert(cloned);
                        }
                    }
                    _ => panic!("Must provide U32 value for replicate pipe!"),
                }
            }
            SelectionKind::Multiple(_) => {
                panic!("Multiple selection kind not supported for replicate")
            }
        };
    }
}

/// A simple node that wraps a node in a 2x oversampler.
///
/// In the future, there will be more rates and an FIR builder.
///
/// For the time being, if you need higher rates, you can design an FIR
/// filter and pass create a node, and create your own pipe, or simply use
/// it as a node. You can also create a subgraph with a different rate as well.
struct Oversample2X;

impl Pipe for Oversample2X {
    fn pipe(&self, view: &mut SelectionView, _: Option<Value>) {
        let selection = view.selection().clone();

        let config = view.config();

        match selection {
            SelectionKind::Single(key) => {
                let node = view.get_node(&key).expect("Could not find key in Pipe!");
                let ports = node.get_node().ports();
                let oversampler = oversample_by_two_factory(
                    node.clone(),
                    ports.audio_out.len(),
                    config.block_size,
                );

                let new_kind = format!("Oversample2X{}", node.node_kind);

                let new_node = LegatoNode::new(node.name.clone(), new_kind, Box::new(oversampler));

                view.replace(key, new_node);
            }
            SelectionKind::Multiple(keys) => {
                for key in keys.iter() {
                    let node = view.get_node(key).expect("Could not find key in Pipe!");
                    let ports = node.get_node().ports();
                    let oversampler = oversample_by_two_factory(
                        node.clone(),
                        ports.audio_out.len(),
                        config.block_size,
                    );

                    let new_kind = format!("Oversample2X{}", node.node_kind);

                    let new_node =
                        LegatoNode::new(node.name.clone(), new_kind, Box::new(oversampler));

                    view.replace(*key, new_node);
                }
            }
        }
    }
}
