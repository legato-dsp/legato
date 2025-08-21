use std::collections::VecDeque;

use indexmap::IndexSet;
use slotmap::{new_key_type, SecondaryMap, SlotMap};

use crate::engine::node::Node;

#[derive(Debug, PartialEq)]
pub enum GraphError {
    BadConnection,
    CycleDetected
}


#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Connection {
    pub source_key: NodeKey,
    pub sink_key: NodeKey,
    pub source_port_index: usize,
    pub sink_port_index: usize
}

new_key_type! { pub struct NodeKey; }

const MAXIUMUM_INPUTS: usize = 8;

pub type AudioNode< const N: usize> = Box<dyn Node<N>>;

// A DAG for grabbing nodes and their deps. via topo sort
pub struct AudioGraph<const N: usize> 
{
    nodes: SlotMap<NodeKey, AudioNode<N>>,
    incoming_edges: SecondaryMap<NodeKey, IndexSet<Connection>>,
    outgoing_edges: SecondaryMap<NodeKey, IndexSet<Connection>>,
    // Pre-allocated work buffers for topo sort
    indegree: SecondaryMap<NodeKey, usize>,
    no_incoming_edges_queue: VecDeque<NodeKey>,
    topo_sorted: Vec<NodeKey>
}

impl< const N: usize> AudioGraph<N> {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            nodes: SlotMap::with_capacity_and_key(capacity),
            incoming_edges: SecondaryMap::with_capacity(capacity),
            outgoing_edges: SecondaryMap::with_capacity(capacity),
            // Pre-allocated work buffers for topo sort
            indegree: SecondaryMap::with_capacity(capacity),
            no_incoming_edges_queue: VecDeque::with_capacity(capacity),
            topo_sorted: Vec::with_capacity(capacity)
        }
    }
    pub fn add_node(&mut self, node: AudioNode<N>) -> NodeKey {
        let key = self.nodes.insert(node);
        self.indegree.insert(key, 0);
        self.incoming_edges.insert(key, IndexSet::with_capacity(MAXIUMUM_INPUTS));
        self.outgoing_edges.insert(key, IndexSet::with_capacity(MAXIUMUM_INPUTS));
        key
    }
    pub fn get_node(&self, key: NodeKey) -> Option<&AudioNode<N>> {
        self.nodes.get(key)
    }
    pub fn get_node_mut(&mut self, key: NodeKey) -> Option<&mut AudioNode<N>>{
        self.nodes.get_mut(key)
    }
    pub fn remove_node(&mut self, key: NodeKey) -> Option<AudioNode<N>> {
        if let Some(item) = self.indegree.get_mut(key){
            *item = 0;
        }
        self.nodes.remove(key)
    }
    pub fn add_edge(&mut self, connection: Connection) -> Result<Connection, GraphError>{
        match self.outgoing_edges.get_mut(connection.source_key) {
            Some(adjacencies) => {
                adjacencies.insert(connection);
            },
            None => return Err(GraphError::BadConnection)
        }
        match self.incoming_edges.get_mut(connection.sink_key) {
            Some(adjacencies) => {
                adjacencies.insert(connection);
            },
            None => return Err(GraphError::BadConnection)
        }
        Ok(connection)
    }
    pub fn get_incoming_nodes(&self, key: NodeKey) -> Option<&IndexSet<Connection>> {
        self.incoming_edges.get(key)
    } 
    pub fn remove_edge(&mut self, connection: Connection) -> Result<(), GraphError> {
        match self.outgoing_edges.get_mut(connection.source_key) {
            Some(adjacencies) => {
                adjacencies.shift_remove(&connection);
            },
            None => return Err(GraphError::BadConnection)
        }
        match self.incoming_edges.get_mut(connection.sink_key) {
            Some(adjacencies) => {
                adjacencies.shift_remove(&connection);
                return Ok(())
            },
            None => Err(GraphError::BadConnection)
        }
    }
    fn invalidate_topo_sort(&mut self) -> Result<&Vec<NodeKey>, GraphError> {
        for item in self.indegree.iter_mut() {
            *item.1 = 0 as usize;
        }

        println!("{:?}", self.indegree);

        for (key, targets) in &self.incoming_edges {
            self.indegree[key] += targets.len();
        }

        println!("{:?}", self.indegree);

        self.no_incoming_edges_queue.clear();

        for (node_key, count) in self.indegree.iter() {
            if *count == 0 {
                self.no_incoming_edges_queue.push_back(node_key);
            }
        }

        self.topo_sorted.clear();
        
        while let Some(node_key) = self.no_incoming_edges_queue.pop_front() {
            self.topo_sorted.push(node_key);
            if let Some(connections) = self.outgoing_edges.get(node_key){
                for con in connections {
                    self.indegree[con.sink_key] -= 1;
                    if self.indegree[con.sink_key] == 0 {
                        self.no_incoming_edges_queue.push_back(con.sink_key);
                    }
                }
            }
        }

        if self.topo_sorted.len() == self.indegree.len() {
            Ok(&self.topo_sorted)
        }
        else {
            Err(GraphError::CycleDetected)
        }
    }
}


#[cfg(test)]
mod test {
    use crate::engine::{graph::{AudioGraph, Connection}, node::Node, port::{Port, PortBehavior, Ported}};
    use crate::engine::audio_context::AudioContext;
    use crate::engine::buffer::Frame;

    #[derive(Default, Debug, PartialEq, Hash)]
    struct ExampleNode {}
    impl Ported for ExampleNode {
        fn get_input_ports(&self) -> &'static [Port] {
            &[
                Port {
                    name: "AUDIO",
                    behavior: PortBehavior::Default,
                    index: 0
                }
            ]
        }
        fn get_output_ports(&self) -> &'static [Port] {
            &[
                Port {
                    name: "AUDIO",
                    behavior: PortBehavior::Default,
                    index: 0
                }
            ]
        }
    }
    impl<const N: usize> Node<N> for ExampleNode {
        fn process(&mut self, ctx: &AudioContext, inputs: &Frame<N>, output: &mut Frame<N>) {
            todo!()
        }
    }

    #[test]
    fn test_topo_sort(){
        let node_a = Box::new(ExampleNode::default());
        let node_b = Box::new(ExampleNode::default());
        let node_c = Box::new(ExampleNode::default());

        let mut graph = AudioGraph::<256>::with_capacity(3);

        let a = graph.add_node(node_a);
        let b = graph.add_node(node_b);
        let c = graph.add_node(node_c);

        let e1 = graph.add_edge(Connection { source_key: a, sink_key: b, sink_port_index: 0, source_port_index: 0 }).expect("Could not add e1");
        let e2 = graph.add_edge(Connection { source_key: b, sink_key: c, sink_port_index: 0, source_port_index: 0 }).expect("Could not add e2");

        assert_eq!(graph.invalidate_topo_sort().expect("Could not topo sort!"), &vec![a,b,c])
    }

    #[test]
    fn test_removal(){
        let node_a = Box::new(ExampleNode::default());
        let node_b = Box::new(ExampleNode::default());
        let node_c = Box::new(ExampleNode::default());

        let mut graph = AudioGraph::<256>::with_capacity(3);

        let b = graph.add_node(node_b);
        let a = graph.add_node(node_a);
        let c = graph.add_node(node_c);

        let e1 = graph.add_edge(Connection { source_key: a, sink_key: b, sink_port_index: 0, source_port_index: 0 }).expect("Could not add edge e1");
        let e2 = graph.add_edge(Connection { source_key: b, sink_key: c, sink_port_index: 0, source_port_index: 0 }).expect("Could not add edge e2");

        assert_eq!(graph.invalidate_topo_sort().expect("Could not topo sort!"), &vec![a,b,c]);

        println!("{:?}", graph.incoming_edges);
        println!("{:?}", graph.outgoing_edges);

        assert_eq!(graph.get_incoming_nodes(b).expect("Node should exist!").get(&e1), Some(&e1));
        assert_eq!(graph.get_incoming_nodes(c).expect("Node should exist!").get(&e2), Some(&e2));

        graph.remove_edge(e1).unwrap();
        graph.remove_edge(e2).unwrap();
        
        assert_eq!(graph.get_incoming_nodes(b).expect("Node should exist!").get(&e1), None);
        assert_eq!(graph.get_incoming_nodes(c).expect("Node should exist!").get(&e2), None);
    }

    #[test]
    fn larger_graph(){
        let node_a = Box::new(ExampleNode::default());
        let node_b = Box::new(ExampleNode::default());
        let node_c = Box::new(ExampleNode::default());
        let node_d = Box::new(ExampleNode::default());
        let node_e = Box::new(ExampleNode::default());

        let mut graph = AudioGraph::<256>::with_capacity(5);

        let a = graph.add_node(node_a);
        let b = graph.add_node(node_b);
        let c = graph.add_node(node_c);
        let d = graph.add_node(node_d);
        let e = graph.add_node(node_e);

        let e1 = graph.add_edge(Connection { source_key: a, sink_key: b, sink_port_index: 0, source_port_index: 0 }).expect("Could not add edge e1");
        let e2 = graph.add_edge(Connection { source_key: b, sink_key: c, sink_port_index: 0, source_port_index: 0 }).expect("Could not add edge e2");
        let e3 = graph.add_edge(Connection { source_key: d, sink_key: c, sink_port_index: 0, source_port_index: 0 }).expect("Could not add edge e3");
        let e4 = graph.add_edge(Connection { source_key: c, sink_key: e, sink_port_index: 0, source_port_index: 0 }).expect("Could not add edge e4");


        assert_eq!(graph.invalidate_topo_sort().expect("Could not topo sort!"), &vec![a,d,b,c,e])
    }
}