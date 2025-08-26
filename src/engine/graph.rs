use std::collections::VecDeque;
use indexmap::IndexSet;
use slotmap::{new_key_type, SecondaryMap, SlotMap};
use crate::engine::node::Node;

#[derive(Debug, PartialEq)]
pub enum GraphError {
    BadConnection,
    CycleDetected,
    NodeDoesNotExist
}

new_key_type! { pub struct NodeKey; }

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Connection {
    pub source_key: NodeKey,
    pub sink_key: NodeKey,
    pub source_port_index: usize,
    pub sink_port_index: usize,
}

const MAXIMUM_INPUTS: usize = 8;

pub type AudioNode<const N: usize> = Box<dyn Node<N> + Send>;

/// A DAG for grabbing nodes and their dependencies via topological sort.
pub struct AudioGraph<const N: usize> {
    nodes: SlotMap<NodeKey, AudioNode<N>>,
    incoming_edges: SecondaryMap<NodeKey, IndexSet<Connection>>,
    outgoing_edges: SecondaryMap<NodeKey, IndexSet<Connection>>,
    // Pre-allocated work buffers for topo sort
    indegree: SecondaryMap<NodeKey, usize>,
    no_incoming_edges_queue: VecDeque<NodeKey>,
    topo_sorted: Vec<NodeKey>,
}

impl<const N: usize> AudioGraph<N> {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            nodes: SlotMap::with_capacity_and_key(capacity),
            incoming_edges: SecondaryMap::with_capacity(capacity),
            outgoing_edges: SecondaryMap::with_capacity(capacity),
            // Pre-allocated work buffers for topo sort

            indegree: SecondaryMap::with_capacity(capacity),
            no_incoming_edges_queue: VecDeque::with_capacity(capacity),
            topo_sorted: Vec::with_capacity(capacity),
        }
    }

    pub fn add_node(&mut self, node: AudioNode<N>) -> NodeKey {
        let key = self.nodes.insert(node);
        self.indegree.insert(key, 0);
        self.incoming_edges
            .insert(key, IndexSet::with_capacity(MAXIMUM_INPUTS));
        self.outgoing_edges
            .insert(key, IndexSet::with_capacity(MAXIMUM_INPUTS));

        let _ = self.invalidate_topo_sort();

        key
    }

    pub fn exists(&self, key: NodeKey) -> bool {
        self.nodes.get(key).is_some()
    }

    #[inline(always)]
    pub fn get_node(&self, key: NodeKey) -> Option<&AudioNode<N>> {
        self.nodes.get(key)
    }

    #[inline(always)]
    pub fn get_node_mut(&mut self, key: &NodeKey) -> Option<&mut AudioNode<N>> {
        self.nodes.get_mut(*key)
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn get_nodes_and_runtime_info(&mut self) -> (&Vec<NodeKey>, &mut SlotMap<NodeKey, AudioNode<N>>, &SecondaryMap<NodeKey, IndexSet<Connection>>) {
        (&self.topo_sorted, &mut self.nodes, &self.incoming_edges)
    }

    /// Removes a node and all edges incident to it.
    pub fn remove_node(&mut self, key: NodeKey) -> Option<AudioNode<N>> {
        if !self.nodes.contains_key(key) {
            return None;
        }

        // Remove edges. TODO: Does the SlotMap repo have something that takes care of this for us?

        if let Some(outgoing) = self.outgoing_edges.remove(key) {
            for con in outgoing.iter() {
                if let Some(in_set) = self.incoming_edges.get_mut(con.sink_key) {
                    in_set.shift_remove(con);
                }
            }
        }

        if let Some(incoming) = self.incoming_edges.remove(key) {
            for con in incoming.iter() {
                if let Some(out_set) = self.outgoing_edges.get_mut(con.source_key) {
                    out_set.shift_remove(con);
                }
            }
        }

        self.indegree.remove(key);
        let node = self.nodes.remove(key);

        self.invalidate_topo_sort().unwrap();

        node
    }

    pub fn add_edge(&mut self, connection: Connection) -> Result<Connection, GraphError> {
        if !self.nodes.contains_key(connection.source_key)
            || !self.nodes.contains_key(connection.sink_key)
        {
            return Err(GraphError::BadConnection);
        }

        match self.outgoing_edges.get_mut(connection.source_key) {
            Some(adjacencies) => {
                adjacencies.insert(connection);
            }
            None => return Err(GraphError::BadConnection),
        }
        match self.incoming_edges.get_mut(connection.sink_key) {
            Some(adjacencies) => {
                adjacencies.insert(connection);
            }
            None => return Err(GraphError::BadConnection),
        }
        self.invalidate_topo_sort()?;
        Ok(connection)
    }

    pub fn incoming_connections(&self, key: NodeKey) -> Option<&IndexSet<Connection>> {
        self.incoming_edges.get(key)
    }

    pub fn outgoing_connections(&self, key: NodeKey) -> Option<&IndexSet<Connection>> {
        self.outgoing_edges.get(key)
    }

    pub fn remove_edge(&mut self, connection: Connection) -> Result<(), GraphError> {
        let mut adj_remove_status = true;
        match self.outgoing_edges.get_mut(connection.source_key) {
            Some(adjacencies) => {
                if !adjacencies.shift_remove(&connection) {
                    adj_remove_status = false;
                }
            }
            None => return Err(GraphError::BadConnection),
        }
        match self.incoming_edges.get_mut(connection.sink_key) {
            Some(adjacencies) => {
                if !adjacencies.shift_remove(&connection) {
                    adj_remove_status = false;
                }
            }
            None => return Err(GraphError::BadConnection),
        }
        if adj_remove_status {
            let _ = self.invalidate_topo_sort().map_err(|_| GraphError::BadConnection);
            Ok(())
        } else {
            Err(GraphError::BadConnection)
        }
    }

    pub fn invalidate_topo_sort(&mut self) -> Result<Vec<NodeKey>, GraphError> {
        // Reset indegrees
        for key in self.nodes.keys() {
            if let Some(v) = self.indegree.get_mut(key) {
                *v = 0;
            } else {
                self.indegree.insert(key, 0);
            }
        }

        // Build indegrees
        for (key, targets) in &self.incoming_edges {
            if self.nodes.contains_key(key) {
                if let Some(count) = self.indegree.get_mut(key) {
                    *count = targets.len();
                }
            }
        }

        self.no_incoming_edges_queue.clear();
        for (node_key, &count) in self.indegree.iter() {
            if count == 0 {
                self.no_incoming_edges_queue.push_back(node_key);
            }
        }

        self.topo_sorted.clear();

        // Kahn's algorithm
        while let Some(node_key) = self.no_incoming_edges_queue.pop_front() {
            self.topo_sorted.push(node_key);
            if let Some(connections) = self.outgoing_edges.get(node_key) {
                for con in connections {
                    if let Some(v) = self.indegree.get_mut(con.sink_key) {
                        *v -= 1;
                        if *v == 0 {
                            self.no_incoming_edges_queue.push_back(con.sink_key);
                        }
                    }
                }
            }
        }

        if self.topo_sorted.len() == self.nodes.len() {
            // I think this is acceptable for the time being, as it should not be happening in realtime, but we can refactor this soon
            Ok(self.topo_sorted.clone())
        } else {
            Err(GraphError::CycleDetected)
        }
    }
}

#[cfg(test)]
mod test {
    use crate::engine::audio_context::AudioContext;
    use crate::engine::buffer::Frame;
    use crate::engine::graph::{AudioGraph, Connection};
    use crate::engine::node::Node;
    use crate::engine::port::{Port, PortBehavior, Ported};
    use crate::engine::graph::GraphError::CycleDetected;
    
    use super::NodeKey;

    #[derive(Default, Debug, PartialEq, Hash)]
    struct ExampleNode {}

    impl Ported for ExampleNode {
        fn get_input_ports(&self) -> &'static [Port] {
            &[Port {
                name: "AUDIO",
                behavior: PortBehavior::Default,
                index: 0,
            }]
        }
        fn get_output_ports(&self) -> &'static [Port] {
            &[Port {
                name: "AUDIO",
                behavior: PortBehavior::Default,
                index: 0,
            }]
        }
    }

    impl<const N: usize> Node<N> for ExampleNode {
        fn process(&mut self, _ctx: &AudioContext, _inputs: &Frame<N>, _output: &mut Frame<N>) {}
    }

    fn assert_is_valid_topo<const N: usize>(g: &mut AudioGraph<N>) {
        let order = g.invalidate_topo_sort().expect("Could not get topo order");

        use std::collections::HashMap;
        let pos: HashMap<NodeKey, usize> = HashMap::<NodeKey, usize>::from_iter(
            order
            .iter()
            .enumerate()
            .map(|(i, v)| { (*v,i) })
        );

        for (src, outs) in &g.outgoing_edges {
            for con in outs.iter() {
                let i = *pos.get(&src).expect("missing src");
                let j = *pos
                    .get(&con.sink_key)
                    .expect("missing sink");
                assert!(i < j, "edge violates topological order");
            }
        }
    }

    #[test]
    fn test_topo_sort_simple_chain() {
        let mut graph = AudioGraph::<256>::with_capacity(3);

        let a = graph.add_node(Box::new(ExampleNode::default()));
        let b = graph.add_node(Box::new(ExampleNode::default()));
        let c = graph.add_node(Box::new(ExampleNode::default()));

        graph
            .add_edge(Connection {
                source_key: a,
                sink_key: b,
                sink_port_index: 0,
                source_port_index: 0,
            })
            .unwrap();
        graph
            .add_edge(Connection {
                source_key: b,
                sink_key: c,
                sink_port_index: 0,
                source_port_index: 0,
            })
            .unwrap();

        assert_is_valid_topo(&mut graph);
    }

    #[test]
    fn test_remove_edges() {
        let mut graph = AudioGraph::<256>::with_capacity(3);

        let a = graph.add_node(Box::new(ExampleNode::default()));
        let b = graph.add_node(Box::new(ExampleNode::default()));
        let c = graph.add_node(Box::new(ExampleNode::default()));

        let e1 = graph
            .add_edge(Connection {
                source_key: a,
                sink_key: b,
                sink_port_index: 0,
                source_port_index: 0,
            })
            .expect("Could not add e1");
        let e2 = graph
            .add_edge(Connection {
                source_key: b,
                sink_key: c,
                sink_port_index: 0,
                source_port_index: 0,
            })
            .expect("Could not add e2");

        // Sanity
        assert!(graph
            .incoming_connections(b)
            .expect("Node should exist!")
            .contains(&e1));
        assert!(graph
            .incoming_connections(c)
            .expect("Node should exist!")
            .contains(&e2));

        graph.remove_edge(e1).unwrap();
        graph.remove_edge(e2).unwrap();

        assert!(!graph
            .incoming_connections(b)
            .expect("Node should exist!")
            .contains(&e1));
        assert!(!graph
            .incoming_connections(c)
            .expect("Node should exist!")
            .contains(&e2));
    }

    #[test]
    fn test_larger_graph_parallel_inputs() {
        let mut graph = AudioGraph::<256>::with_capacity(5);

        let a = graph.add_node(Box::new(ExampleNode::default()));
        let b = graph.add_node(Box::new(ExampleNode::default()));
        let c = graph.add_node(Box::new(ExampleNode::default()));
        let d = graph.add_node(Box::new(ExampleNode::default()));
        let e = graph.add_node(Box::new(ExampleNode::default()));

        graph
            .add_edge(Connection {
                source_key: a,
                sink_key: b,
                sink_port_index: 0,
                source_port_index: 0,
            })
            .unwrap();
        graph
            .add_edge(Connection {
                source_key: b,
                sink_key: c,
                sink_port_index: 0,
                source_port_index: 0,
            })
            .unwrap();
        graph
            .add_edge(Connection {
                source_key: d,
                sink_key: c,
                sink_port_index: 0,
                source_port_index: 0,
            })
            .unwrap();
        graph
            .add_edge(Connection {
                source_key: c,
                sink_key: e,
                sink_port_index: 0,
                source_port_index: 0,
            })
            .unwrap();

        assert_is_valid_topo(&mut graph);
    }

    #[test]
    fn test_cycle_detection_two_node_cycle() {
        let mut graph = AudioGraph::<256>::with_capacity(2);
        let a = graph.add_node(Box::new(ExampleNode::default()));
        let b = graph.add_node(Box::new(ExampleNode::default()));

        let _ = graph
            .add_edge(Connection {
                source_key: a,
                sink_key: b,
                sink_port_index: 0,
                source_port_index: 0,
            })
            .unwrap();
        // Bad edge, contains Err from cycle
        let _ = graph
            .add_edge(Connection {
                source_key: b,
                sink_key: a,
                sink_port_index: 0,
                source_port_index: 0,
            });

        let res = graph.invalidate_topo_sort();
        assert_eq!(res, Err(CycleDetected));
    }

    #[test]
    fn test_cycle_detection_self_loop() {
        let mut graph = AudioGraph::<256>::with_capacity(1);
        let a = graph.add_node(Box::new(ExampleNode::default()));
        let res = graph.add_edge(Connection { source_key: a, sink_key: a, sink_port_index: 0, source_port_index: 0});
        assert_eq!(res, Err(CycleDetected));
    }
    
    #[test]
    fn single_node_order(){
        let mut graph = AudioGraph::<256>::with_capacity(1);
        let a = graph.add_node(Box::new(ExampleNode::default()));

        assert_eq!(graph.topo_sorted, vec![a])
    }

    #[test]
    fn test_remove_node_cleans_edges_and_topo() {
        let mut graph = AudioGraph::<256>::with_capacity(3);
        let a = graph.add_node(Box::new(ExampleNode::default()));
        let b = graph.add_node(Box::new(ExampleNode::default()));
        let c = graph.add_node(Box::new(ExampleNode::default()));

        graph
            .add_edge(Connection {
                source_key: a,
                sink_key: b,
                sink_port_index: 0,
                source_port_index: 0,
            })
            .unwrap();
        graph
            .add_edge(Connection {
                source_key: b,
                sink_key: c,
                sink_port_index: 0,
                source_port_index: 0,
            })
            .unwrap();

        // Remove middle node; edges should be purged
        let _ = graph.remove_node(b).expect("node existed");

        assert_is_valid_topo(&mut graph);

        // No incoming/outgoing entries left for the removed node
        assert!(graph.incoming_connections(b).is_none());
        assert!(graph.outgoing_connections(b).is_none());
    }

    #[test]
    fn test_add_edge_rejects_missing_endpoints() {
        let mut graph = AudioGraph::<256>::with_capacity(2);
        let a = graph.add_node(Box::new(ExampleNode::default()));

        // Spoof a bad key
        let nonexistent_key = {
            let temp = graph.add_node(Box::new(ExampleNode::default()));
            let _ = graph.remove_node(temp);
            temp
        };
        let res = graph.add_edge(Connection {
            source_key: a,
            sink_key: nonexistent_key,
            sink_port_index: 0,
            source_port_index: 0,
        });
        assert_eq!(res.unwrap_err(), crate::engine::graph::GraphError::BadConnection);
    }
}
