use indexmap::IndexSet;
use slotmap::{SecondaryMap, SlotMap, new_key_type};
use std::{collections::VecDeque};

use crate::nodes::{Node, ports::PortRate};

#[derive(Debug, PartialEq)]
pub enum GraphError {
    BadConnection,
    CycleDetected,
    NodeDoesNotExist,
}

new_key_type! { pub struct NodeKey; }

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct ConnectionEntry {
    pub node_key: NodeKey,
    pub port_index: usize,
    pub port_rate: PortRate,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Connection {
    pub source: ConnectionEntry,
    pub sink: ConnectionEntry,
}

const MAXIMUM_INPUTS: usize = 8;

pub type AudioNode = Box<dyn Node + Send>;

/// A DAG for grabbing nodes and their dependencies via topological sort.
pub struct AudioGraph
{
    nodes: SlotMap<NodeKey, AudioNode>,
    incoming_edges: SecondaryMap<NodeKey, IndexSet<Connection>>,
    outgoing_edges: SecondaryMap<NodeKey, IndexSet<Connection>>,
    // Pre-allocated work buffers for topo sort
    indegree: SecondaryMap<NodeKey, usize>,
    no_incoming_edges_queue: VecDeque<NodeKey>,
    topo_sorted: Vec<NodeKey>,
}

impl AudioGraph
{
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

    pub fn add_node(&mut self, node: AudioNode) -> NodeKey {
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
    pub fn get_node(&self, key: NodeKey) -> Option<&AudioNode> {
        self.nodes.get(key)
    }

    #[inline(always)]
    pub fn get_node_mut(&mut self, key: &NodeKey) -> Option<&mut AudioNode> {
        self.nodes.get_mut(*key)
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn get_sort_order_nodes_and_runtime_info(
        &mut self,
    ) -> (
        &Vec<NodeKey>,
        &mut SlotMap<NodeKey, AudioNode>,
        &SecondaryMap<NodeKey, IndexSet<Connection>>,
    ) {
        (&self.topo_sorted, &mut self.nodes, &self.incoming_edges)
    }

    /// Removes a node and all edges incident to it.
    pub fn remove_node(&mut self, key: NodeKey) -> Option<AudioNode> {
        if !self.nodes.contains_key(key) {
            return None;
        }

        // Remove edges. TODO: Does the SlotMap repo have something that takes care of this for us?

        if let Some(outgoing) = self.outgoing_edges.remove(key) {
            for con in outgoing.iter() {
                if let Some(in_set) = self.incoming_edges.get_mut(con.sink.node_key) {
                    in_set.shift_remove(con);
                }
            }
        }

        if let Some(incoming) = self.incoming_edges.remove(key) {
            for con in incoming.iter() {
                if let Some(out_set) = self.outgoing_edges.get_mut(con.source.node_key) {
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
        if !self.nodes.contains_key(connection.source.node_key)
            || !self.nodes.contains_key(connection.sink.node_key)
        {
            return Err(GraphError::BadConnection);
        }

        match self.outgoing_edges.get_mut(connection.source.node_key) {
            Some(adjacencies) => {
                adjacencies.insert(connection);
            }
            None => return Err(GraphError::BadConnection),
        }
        match self.incoming_edges.get_mut(connection.sink.node_key) {
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
        match self.outgoing_edges.get_mut(connection.source.node_key) {
            Some(adjacencies) => {
                if !adjacencies.shift_remove(&connection) {
                    adj_remove_status = false;
                }
            }
            None => return Err(GraphError::BadConnection),
        }
        match self.incoming_edges.get_mut(connection.sink.node_key) {
            Some(adjacencies) => {
                if !adjacencies.shift_remove(&connection) {
                    adj_remove_status = false;
                }
            }
            None => return Err(GraphError::BadConnection),
        }
        if adj_remove_status {
            let _ = self
                .invalidate_topo_sort()
                .map_err(|_| GraphError::BadConnection);
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
                    if let Some(v) = self.indegree.get_mut(con.sink.node_key) {
                        *v -= 1;
                        if *v == 0 {
                            self.no_incoming_edges_queue.push_back(con.sink.node_key);
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
    use crate::{nodes::{Node, ports::{PortMeta, Ported, Ports, PortRate}}, runtime::graph::{AudioGraph, NodeKey}};
    use super::*;

    struct MonoExample {
        ports: Ports
    }

    impl Default for MonoExample {
        fn default() -> Self {
            Self {
                ports: Ports { 
                    audio_in: Some(vec![PortMeta { name: "in", index: 0}]), 
                    audio_out: Some(vec![PortMeta { name: "out", index: 0} ]), 
                    control_in: None, control_out: 
                    None 
                }
            }
        }
    }

    impl Ported for MonoExample {
        fn get_ports(&self) -> &Ports {
            &self.ports
        }
    }

    impl Node for MonoExample {
        fn process(&mut self, ctx: &mut crate::runtime::context::AudioContext, 
                ai: &[ &[f32] ],
                ao: &mut[ &mut[f32] ],
                ci: &[ &[f32] ],
                co: &mut[ &mut[f32] ],
            ) {}
    }
   
    fn assert_is_valid_topo(g: &mut AudioGraph)
    {
        let order = g.invalidate_topo_sort().expect("Could not get topo order");

        use std::collections::HashMap;
        let pos: HashMap<NodeKey, usize> =
            HashMap::<NodeKey, usize>::from_iter(order.iter().enumerate().map(|(i, v)| (*v, i)));

        for (src, outs) in &g.outgoing_edges {
            for con in outs.iter() {
                let i = *pos.get(&src).expect("missing src");
                let j = *pos.get(&con.sink.node_key).expect("missing sink");
                assert!(i < j, "edge violates topological order");
            }
        }
    }

    #[test]
    fn test_topo_sort_simple_chain() {
        let mut graph: AudioGraph = AudioGraph::with_capacity(3);

        let a = graph.add_node(Box::new(MonoExample::default()));
        let b = graph.add_node(Box::new(MonoExample::default()));
        let c = graph.add_node(Box::new(MonoExample::default()));

        graph
            .add_edge(Connection {
                source: ConnectionEntry {
                    node_key: a,
                    port_index: 0,
                    port_rate: PortRate::Audio,
                },
                sink: ConnectionEntry {
                    node_key: b,
                    port_index: 0,
                    port_rate: PortRate::Audio,
                },
            })
            .unwrap();
        graph
            .add_edge(Connection {
                source: ConnectionEntry {
                    node_key: b,
                    port_index: 0,
                    port_rate: PortRate::Audio,
                },
                sink: ConnectionEntry {
                    node_key: c,
                    port_index: 0,
                    port_rate: PortRate::Audio,
                },
            })
            .unwrap();

        assert_is_valid_topo(&mut graph);
    }

    #[test]
    fn test_remove_edges() {
        let mut graph = AudioGraph::with_capacity(3);

        let a = graph.add_node(Box::new(MonoExample::default()));
        let b = graph.add_node(Box::new(MonoExample::default()));
        let c = graph.add_node(Box::new(MonoExample::default()));

        let e1 = graph
            .add_edge(Connection {
                source: ConnectionEntry {
                    node_key: a,
                    port_index: 0,
                    port_rate: PortRate::Audio,
                },
                sink: ConnectionEntry {
                    node_key: b,
                    port_index: 0,
                    port_rate: PortRate::Audio,
                },
            })
            .expect("Could not add e1");
        let e2 = graph
            .add_edge(Connection {
                source: ConnectionEntry {
                    node_key: b,
                    port_index: 0,
                    port_rate: PortRate::Audio,
                },
                sink: ConnectionEntry {
                    node_key: c,
                    port_index: 0,
                    port_rate: PortRate::Audio,
                },
            })
            .expect("Could not add e2");

        // Sanity
        assert!(
            graph
                .incoming_connections(b)
                .expect("Node should exist!")
                .contains(&e1)
        );
        assert!(
            graph
                .incoming_connections(c)
                .expect("Node should exist!")
                .contains(&e2)
        );

        graph.remove_edge(e1).unwrap();
        graph.remove_edge(e2).unwrap();

        assert!(
            !graph
                .incoming_connections(b)
                .expect("Node should exist!")
                .contains(&e1)
        );
        assert!(
            !graph
                .incoming_connections(c)
                .expect("Node should exist!")
                .contains(&e2)
        );
    }

    #[test]
    fn test_larger_graph_parallel_inputs() {
        let mut graph = AudioGraph::with_capacity(5);

        let a = graph.add_node(Box::new(MonoExample::default()));
        let b = graph.add_node(Box::new(MonoExample::default()));
        let c = graph.add_node(Box::new(MonoExample::default()));
        let d = graph.add_node(Box::new(MonoExample::default()));
        let e = graph.add_node(Box::new(MonoExample::default()));

        graph
            .add_edge(Connection {
                source: ConnectionEntry {
                    node_key: a,
                    port_index: 0,
                    port_rate: PortRate::Audio,
                },
                sink: ConnectionEntry {
                    node_key: b,
                    port_index: 0,
                    port_rate: PortRate::Audio,
                },
            })
            .unwrap();
        graph
            .add_edge(Connection {
                source: ConnectionEntry {
                    node_key: b,
                    port_index: 0,
                    port_rate: PortRate::Audio,
                },
                sink: ConnectionEntry {
                    node_key: c,
                    port_index: 0,
                    port_rate: PortRate::Audio,
                },
            })
            .unwrap();
        graph
            .add_edge(Connection {
                source: ConnectionEntry {
                    node_key: d,
                    port_index: 0,
                    port_rate: PortRate::Audio,
                },
                sink: ConnectionEntry {
                    node_key: c,
                    port_index: 0,
                    port_rate: PortRate::Audio,
                },
            })
            .unwrap();
        graph
            .add_edge(Connection {
                source: ConnectionEntry {
                    node_key: c,
                    port_index: 0,
                    port_rate: PortRate::Audio,
                },
                sink: ConnectionEntry {
                    node_key: e,
                    port_index: 0,
                    port_rate: PortRate::Audio,
                },
            })
            .unwrap();

        assert_is_valid_topo(&mut graph);
    }

    #[test]
    fn test_cycle_detection_two_node_cycle() {
        let mut graph = AudioGraph::with_capacity(2);
        let a = graph.add_node(Box::new(MonoExample::default()));
        let b = graph.add_node(Box::new(MonoExample::default()));

        let _ = graph
            .add_edge(Connection {
                source: ConnectionEntry {
                    node_key: a,
                    port_index: 0,
                    port_rate: PortRate::Audio,
                },
                sink: ConnectionEntry {
                    node_key: b,
                    port_index: 0,
                    port_rate: PortRate::Audio,
                },
            })
            .unwrap();
        // Should return error from cycle
        let _ = graph.add_edge(Connection {
            source: ConnectionEntry {
                node_key: b,
                port_index: 0,
                port_rate: PortRate::Audio,
            },
            sink: ConnectionEntry {
                node_key: a,
                port_index: 0,
                port_rate: PortRate::Audio,
            },
        });

        let res = graph.invalidate_topo_sort();
        assert_eq!(res, Err(GraphError::CycleDetected));
    }

    #[test]
    fn test_cycle_detection_self_loop() {
        let mut graph = AudioGraph::with_capacity(1);
        let a = graph.add_node(Box::new(MonoExample::default()));
        let res = graph.add_edge(Connection {
            source: ConnectionEntry {
                node_key: a,
                port_index: 0,
                port_rate: PortRate::Audio,
            },
            sink: ConnectionEntry {
                node_key: a,
                port_index: 0,
                port_rate: PortRate::Audio,
            },
        });
        assert_eq!(res, Err(GraphError::CycleDetected));
    }

    #[test]
    fn single_node_order() {
        let mut graph = AudioGraph::with_capacity(1);
        let a = graph.add_node(Box::new(MonoExample::default()));

        assert_eq!(graph.topo_sorted, vec![a])
    }

    #[test]
    fn test_remove_node_cleans_edges_and_topo() {
        let mut graph = AudioGraph::with_capacity(3);
        let a = graph.add_node(Box::new(MonoExample::default()));
        let b = graph.add_node(Box::new(MonoExample::default()));
        let c = graph.add_node(Box::new(MonoExample::default()));

        graph
            .add_edge(Connection {
                source: ConnectionEntry {
                    node_key: a,
                    port_index: 0,
                    port_rate: PortRate::Audio,
                },
                sink: ConnectionEntry {
                    node_key: b,
                    port_index: 0,
                    port_rate: PortRate::Audio,
                },
            })
            .unwrap();
        graph
            .add_edge(Connection {
                source: ConnectionEntry {
                    node_key: b,
                    port_index: 0,
                    port_rate: PortRate::Audio,
                },
                sink: ConnectionEntry {
                    node_key: c,
                    port_index: 0,
                    port_rate: PortRate::Audio,
                },
            })
            .unwrap();

        let _ = graph.remove_node(b).expect("node existed");

        assert_is_valid_topo(&mut graph);

        assert!(graph.incoming_connections(b).is_none());
        assert!(graph.outgoing_connections(b).is_none());
    }

    #[test]
    fn test_add_edge_rejects_missing_endpoints() {
        let mut graph = AudioGraph::with_capacity(2);
        let a = graph.add_node(Box::new(MonoExample::default()));

        // Add a bad key, should throw error when we add an edge
        let nonexistent_key = {
            let temp = graph.add_node(Box::new(MonoExample::default()));
            let _ = graph.remove_node(temp);
            temp
        };
        let res = graph.add_edge(Connection {
            source: ConnectionEntry {
                node_key: a,
                port_index: 0,
                port_rate: PortRate::Audio,
            },
            sink: ConnectionEntry {
                node_key: nonexistent_key,
                port_index: 0,
                port_rate: PortRate::Audio,
            },
        });
        assert_eq!(
            res.unwrap_err(),
            GraphError::BadConnection
        );
    }
}
