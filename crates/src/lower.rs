use crate::ir::*;
use indexmap::IndexMap;
use std::collections::HashMap;

const MAXIMUM_DEPTH: u8 = 16;

// The last 20% and a number of tests were pushed through with LLM usage, I would appreciate a second eye down the line

struct NodeInfo {
    /// FQN to use when this symbol is the source of a connection.
    source_fqn: String,
    /// A map from the virtual port name, to the FQN and Port definition
    virtual_inputs: IndexMap<String, (String, Port)>
}

#[derive(Default, Debug)]
pub struct Lowerer {
    registry: HashMap<String, Macro>,
}

impl Lowerer {
    pub fn lower(&mut self, ast: Ast) -> IR {
        let mut ir = IR::default();
        ir.sink = ast.sink.clone();

        for item in ast.macros {
            self.registry.insert(item.name.clone(), item);
        }

        let mut scope_map: HashMap<String, DeclarationScope> = HashMap::new();
        let mut top_symbols: HashMap<String, NodeInfo> = HashMap::new();

        for scope in &ast.declarations {
            for decl in &scope.declarations {
                let alias = decl.alias.clone().unwrap_or_else(|| decl.node_type.clone());

                if let Some(m) = self.registry.get(&decl.node_type).cloned() {
                    let info = self.expand_macro(
                        &m,
                        &alias,
                        "",
                        &decl.params.clone().unwrap_or_default(),
                        &mut ir,
                        &mut scope_map,
                        0,
                    );
                    top_symbols.insert(alias, info);
                } else {
                    let mut leaf = decl.clone();
                    leaf.alias = Some(alias.clone());

                    scope_map
                        .entry(scope.namespace.clone())
                        .or_insert_with(|| DeclarationScope {
                            namespace: scope.namespace.clone(),
                            declarations: Vec::new(),
                        })
                        .declarations
                        .push(leaf);

                    top_symbols.insert(
                        alias.clone(),
                        NodeInfo {
                            source_fqn: alias,
                            virtual_inputs: IndexMap::new(),
                        },
                    );
                }
            }
        }

        // resolve top lvl connections
        for conn in &ast.connections {
            if let Some(resolved) = Self::resolve_connection(conn, &top_symbols) {
                ir.connections.push(resolved);
            }
        }

        ir.declarations = scope_map.into_values().collect();
        ir
    }

    fn expand_macro(
        &mut self,
        m: &Macro,
        instance_name: &str,
        parent_prefix: &str,
        params: &Object,
        ir: &mut IR,
        scope_map: &mut HashMap<String, DeclarationScope>,
        depth: u8,
    ) -> NodeInfo {
        if depth > MAXIMUM_DEPTH {
            panic!(
                "Max macro depth ({}) exceeded at {}::{}",
                MAXIMUM_DEPTH, parent_prefix, instance_name
            );
        }

        let current_prefix = if parent_prefix.is_empty() {
            instance_name.to_string()
        } else {
            format!("{}.{}", parent_prefix, instance_name)
        };

        // Merge default params, then override with call-site params
        let mut current_params = m.default_params.clone().unwrap_or_default();
        for (k, v) in params {
            current_params.insert(k.clone(), v.clone());
        }

        // Build the local symbol table for this macro's scope
        let mut local_symbols: HashMap<String, NodeInfo> = HashMap::new();

        for scope in &m.declarations.clone() {
            for decl in &scope.declarations {
                let local_alias = decl.alias.as_ref().unwrap_or(&decl.node_type).clone();

                if let Some(inner_macro) = self.registry.get(&decl.node_type).cloned() {
                    let mut inner_params = decl.params.clone().unwrap_or_default();
                    self.resolve_templates(&mut inner_params, &current_params);

                    let info = self.expand_macro(
                        &inner_macro,
                        &local_alias,
                        &current_prefix,
                        &inner_params,
                        ir,
                        scope_map,
                        depth + 1,
                    );
                    local_symbols.insert(local_alias, info);
                } else {
                    let fqn = format!("{}.{}", current_prefix, local_alias);
                    let mut leaf = decl.clone();
                    leaf.alias = Some(fqn.clone());

                    if let Some(ref mut p) = leaf.params {
                        self.resolve_templates(p, &current_params);
                    }

                    scope_map
                        .entry(scope.namespace.clone())
                        .or_insert_with(|| DeclarationScope {
                            namespace: scope.namespace.clone(),
                            declarations: Vec::new(),
                        })
                        .declarations
                        .push(leaf);

                    local_symbols.insert(
                        local_alias,
                        NodeInfo {
                            source_fqn: fqn,
                            virtual_inputs: IndexMap::new(),
                        },
                    );
                }
            }
        }

        let mut virtual_inputs: IndexMap<String, (String, Port)> = IndexMap::new();

        for conn in &m.connections {
            // Virtual connection
            if m.virtual_ports_in.contains(&conn.source.node) {
                let target_info = local_symbols
                    .get(&conn.sink.node)
                    .unwrap_or_else(|| {
                        panic!(
                            "Virtual port '{}' routes to unknown node '{}' in macro '{}'",
                            conn.source.node, conn.sink.node, m.name
                        )
                    });

               let (target_fqn, target_port) = match &conn.sink.port {
                    Port::Named(port_name) => {
                        if let Some((nested_fqn, nested_port)) = target_info.virtual_inputs.get(port_name) {
                            // Follow through to the actual leaf port on the inner macro
                            (nested_fqn.clone(), nested_port.clone())
                        } else {
                            (target_info.source_fqn.clone(), conn.sink.port.clone())
                        }
                    }
                    _ => (target_info.source_fqn.clone(), conn.sink.port.clone()),
                };

                virtual_inputs.insert(
                    conn.source.node.clone(),
                    (target_fqn, target_port),
                );
            } 
            // Normal connection
            else {
                if let Some(resolved) = Self::resolve_connection(conn, &local_symbols) {
                    ir.connections.push(resolved);
                }
            }
        }

        // The sink FQN that will exist for future connections
        let sink_info = local_symbols
            .get(&m.sink)
            .unwrap_or_else(|| panic!("Sink alias '{}' not found in macro '{}'", m.sink, m.name));

        NodeInfo {
            source_fqn: sink_info.source_fqn.clone(),
            virtual_inputs,
        }
    }

    fn resolve_connection(
        conn: &Connection,
        symbols: &HashMap<String, NodeInfo>,
    ) -> Option<Connection> {
        let src_info = symbols
            .get(&conn.source.node)
            .unwrap_or_else(|| panic!("Source node '{}' not found in scope", conn.source.node));

        let snk_info = symbols
            .get(&conn.sink.node)
            .unwrap_or_else(|| panic!("Sink node '{}' not found in scope", conn.sink.node));

        // for macros, we always use the sink leaf, otherwise it's a normal fqn path
        let source_fqn = src_info.source_fqn.clone();

        // if the target has virtual inputs, the port MUST be named
        let (sink_fqn, sink_port) = if snk_info.virtual_inputs.is_empty() {
            (snk_info.source_fqn.clone(), conn.sink.port.clone())
        } else {
            match &conn.sink.port {
                Port::Named(port_name) => {
                    let (target_fqn, target_port) = snk_info
                        .virtual_inputs
                        .get(port_name)
                        .unwrap_or_else(|| {
                            panic!(
                                "Node '{}' has no virtual input port '{}'",
                                conn.sink.node, port_name
                            )
                        });
                    (target_fqn.clone(), target_port.clone())
                }
                // Specifically using an index set so we can select the virtual port in order
                Port::Index(i) => {
                    let (target_fqn, target_port) = snk_info
                        .virtual_inputs
                        .get_index(*i)
                        .map(|(_, v)| v)
                        .unwrap_or_else(|| panic!(
                            "Node '{}' has no virtual input at index {}",
                            conn.sink.node, i
                        ));
                    (target_fqn.clone(), target_port.clone())
                }
                Port::None => {
                    // Automap to sink
                    (snk_info.source_fqn.clone(), Port::None)
                }
                Port::Slice(_, _) => panic!("Slicing not yet supported on virtual ports")
            }
        };

        Some(Connection {
            source: Endpoint {
                node: source_fqn,
                port: conn.source.port.clone(),
            },
            sink: Endpoint {
                node: sink_fqn,
                port: sink_port,
            },
        })
    }

    fn resolve_templates(&self, params: &mut Object, lookup: &Object) {
        for val in params.values_mut() {
            if let Value::Template(tpl) = val {
                let key = tpl.trim_start_matches('$');
                if let Some(replacement) = lookup.get(key) {
                    *val = replacement.clone();
                }
            }
        }
    }
}


#[macro_export]
macro_rules! object {
    () => {
        BTreeMap::new()
    };
    ( $($key:expr => template $val:expr),* $(,)? ) => {
        {
            use std::collections::BTreeMap;
            let mut _map = BTreeMap::new();
            $(
                _map.insert($key.to_string(), $crate::ir::Value::Template($val.to_string()));
            )*
            _map
        }
    };
    ( $($key:expr => $value:expr),* $(,)? ) => {
        {
            use std::collections::BTreeMap;
            let mut _map = BTreeMap::new();
            $(
                _map.insert($key.to_string(), $crate::ir::Value::from($value));
            )*
            _map
        }
    };
}

#[cfg(test)]
mod tests {
    use indexmap::IndexSet;

    use super::*;

    fn make_voice_macro() -> Macro {
        Macro {
            name: "voice".into(),
            default_params: Some(object! {
                "freq" => 440.0f32,
                "attack" => 100.0f32,
                "release" => 500.0f32
            }),
            virtual_ports_in: {
                let mut s = IndexSet::new();
                s.insert("gate".into());
                s.insert("freq_in".into());
                s
            },
            declarations: vec![DeclarationScope {
                namespace: "audio".into(),
                declarations: vec![
                    NodeDeclaration {
                        node_type: "sine".into(),
                        alias: Some("osc".into()),
                        params: Some(object! { "freq" => Value::Template("$freq".into()) }),
                        pipes: vec![],
                    },
                    NodeDeclaration {
                        node_type: "adsr".into(),
                        alias: Some("env".into()),
                        params: Some(object! {
                            "attack" => Value::Template("$attack".into()),
                            "release" => Value::Template("$release".into())
                        }),
                        pipes: vec![],
                    },
                ],
            }],
            connections: vec![
                Connection {
                    source: Endpoint { node: "freq_in".into(), port: Port::None },
                    sink: Endpoint { node: "osc".into(), port: Port::Named("freq".into()) },
                },
                Connection {
                    source: Endpoint { node: "gate".into(), port: Port::None },
                    sink: Endpoint { node: "env".into(), port: Port::Named("gate".into()) },
                },
                Connection {
                    source: Endpoint { node: "osc".into(), port: Port::None },
                    sink: Endpoint { node: "env".into(), port: Port::Index(1) },
                },
            ],
            sink: "env".into(),
        }
    }

    #[test]
    fn test_virtual_port_routing_recorded() {
        let ast = Ast {
            macros: vec![make_voice_macro()],
            declarations: vec![DeclarationScope {
                namespace: "audio".into(),
                declarations: vec![NodeDeclaration {
                    node_type: "voice".into(),
                    alias: Some("v1".into()),
                    params: Some(object!("freq" => 420.0)),
                    pipes: vec![],
                }],
            }],
            ..Default::default()
        };

        let ir = IR::from(ast);

        dbg!(&ir);

        // Correct node size
        assert_eq!(ir.declarations[0].declarations.len(), 2);
        // make sure the param got passed in
        assert_eq!(*ir.declarations[0]
            .declarations.iter()
            .find(|x| x.alias.as_ref().unwrap() == "v1.osc").unwrap()
            .params.as_ref().unwrap()
            .get("freq").unwrap()
        , Value::F32(420.0));

        // we should only have an interior connection from osc -> env
        assert_eq!(ir.connections.len(), 1);
        let conn = &ir.connections[0];
        assert_eq!(conn.source.node, "v1.osc");
        assert_eq!(conn.sink.node, "v1.env");
        assert_eq!(conn.sink.port, Port::Index(1));
        
    }

    #[test]
    fn test_external_connection_through_virtual_port() {
        let ast = Ast {
            macros: vec![make_voice_macro()],
            source: None,
            declarations: vec![
                DeclarationScope {
                    namespace: "audio".into(),
                    declarations: vec![NodeDeclaration {
                        node_type: "voice".into(),
                        alias: Some("v1".into()),
                        params: None,
                        pipes: vec![],
                    }],
                },
                DeclarationScope {
                    namespace: "midi".into(),
                    declarations: vec![NodeDeclaration {
                        node_type: "poly_voice".into(),
                        alias: Some("poly".into()),
                        params: None,
                        pipes: vec![],
                    }],
                },
            ],
            connections: vec![
                Connection {
                    source: Endpoint { node: "poly".into(), port: Port::Named("freq".into()) },
                    sink: Endpoint { node: "v1".into(), port: Port::Named("freq_in".into()) },
                },
                Connection {
                    source: Endpoint { node: "poly".into(), port: Port::Named("gate".into()) },
                    sink: Endpoint { node: "v1".into(), port: Port::Named("gate".into()) },
                },
            ],
            sink: "v1".into(),
        };

        let ir = IR::from(ast);
        dbg!(&ir);

        // macro connection plus the two external connections
        assert_eq!(ir.connections.len(), 3);

        let freq_conn = ir.connections.iter()
            .find(|c| c.source.node == "poly" && c.source.port == Port::Named("freq".into()))
            .expect("freq connection not found");

        assert_eq!(freq_conn.sink.node, "v1.osc");
        assert_eq!(freq_conn.sink.port, Port::Named("freq".into()));

        let gate_conn = ir.connections.iter()
            .find(|c| c.source.node == "poly" && c.source.port == Port::Named("gate".into()))
            .expect("gate connection not found");

        assert_eq!(gate_conn.sink.node, "v1.env");
        assert_eq!(gate_conn.sink.port, Port::Named("gate".into()));
    }

    #[test]
    fn test_multiple_patch_instances_get_distinct_fqns() {
        let ast = Ast {
            macros: vec![make_voice_macro()],
            declarations: vec![DeclarationScope {
                namespace: "audio".into(),
                declarations: vec![
                    NodeDeclaration {
                        node_type: "voice".into(),
                        alias: Some("v1".into()),
                        params: Some(object! { "freq" => 440.0f32 }),
                        pipes: vec![],
                    },
                    NodeDeclaration {
                        node_type: "voice".into(),
                        alias: Some("v2".into()),
                        params: Some(object! { "freq" => 880.0f32 }),
                        pipes: vec![],
                    },
                ],
            }],
            ..Default::default()
        };

        let ir = IR::from(ast);
        dbg!(&ir);

        // Each instance produces its own osc and env leaf
        let all_aliases: Vec<&str> = ir.declarations.iter()
            .flat_map(|s| s.declarations.iter())
            .filter_map(|d| d.alias.as_deref())
            .collect();

        assert!(all_aliases.contains(&"v1.osc"), "missing v1.osc");
        assert!(all_aliases.contains(&"v1.env"), "missing v1.env");
        assert!(all_aliases.contains(&"v2.osc"), "missing v2.osc");
        assert!(all_aliases.contains(&"v2.env"), "missing v2.env");

        // v2.osc should have freq 880
        let v2_osc = ir.declarations.iter()
            .flat_map(|s| s.declarations.iter())
            .find(|d| d.alias.as_deref() == Some("v2.osc"))
            .expect("v2.osc not found");

        assert_eq!(
            v2_osc.params.as_ref().unwrap().get("freq"),
            Some(&Value::F32(880.0))
        );
    }

    #[test]
    fn test_patch_audio_passthrough_via_sink() {
        // When no virtual port is named, connecting patch >> next_node
        // should wire through the patch's sink leaf
        let ast = Ast {
            macros: vec![make_voice_macro()],
            source: None,
            declarations: vec![
                DeclarationScope {
                    namespace: "audio".into(),
                    declarations: vec![
                        NodeDeclaration {
                            node_type: "voice".into(),
                            alias: Some("v1".into()),
                            params: None,
                            pipes: vec![],
                        },
                        NodeDeclaration {
                            node_type: "track_mixer".into(),
                            alias: Some("mixer".into()),
                            params: None,
                            pipes: vec![],
                        },
                    ],
                },
            ],
            connections: vec![
                Connection {
                    source: Endpoint { node: "v1".into(), port: Port::None },
                    sink: Endpoint { node: "mixer".into(), port: Port::None },
                },
            ],
            sink: "mixer".into(),
        };

        let ir = IR::from(ast);

        let passthrough = ir.connections.iter()
            .find(|c| c.sink.node == "mixer")
            .expect("passthrough connection not found");

        // Source should be the sink leaf of the voice
        assert_eq!(passthrough.source.node, "v1.env");
    }

    #[test]
    fn test_nested_macro_virtual_ports_and_connections() {
        // Inner macro: fm_osc
        //   virtual in: freq_in -> modulator.freq, carrier.freq
        //   internal connection: modulator -> carrier[0]
        //   sink: carrier
        let fm_osc_macro = Macro {
            name: "fm_osc".into(),
            default_params: Some(object! {
                "freq" => 440.0f32,
                "mod_freq" => 880.0f32
            }),
            virtual_ports_in: {
                let mut s = IndexSet::new();
                s.insert("freq_in".into());
                s
            },
            declarations: vec![DeclarationScope {
                namespace: "audio".into(),
                declarations: vec![
                    NodeDeclaration {
                        node_type: "sine".into(),
                        alias: Some("modulator".into()),
                        params: Some(object! { "freq" => Value::Template("$mod_freq".into()) }),
                        pipes: vec![],
                    },
                    NodeDeclaration {
                        node_type: "sine".into(),
                        alias: Some("carrier".into()),
                        params: Some(object! { "freq" => Value::Template("$freq".into()) }),
                        pipes: vec![],
                    },
                ],
            }],
            connections: vec![
                Connection {
                    source: Endpoint { node: "freq_in".into(), port: Port::None },
                    sink: Endpoint { node: "carrier".into(), port: Port::Named("freq".into()) },
                },
                Connection {
                    source: Endpoint { node: "modulator".into(), port: Port::None },
                    sink: Endpoint { node: "carrier".into(), port: Port::Index(0) },
                },
            ],
            sink: "carrier".into(),
        };

        let voice_macro = Macro {
            name: "voice".into(),
            default_params: Some(object! {
                "freq" => 440.0f32,
                "attack" => 100.0f32
            }),
            virtual_ports_in: {
                let mut s = IndexSet::new();
                s.insert("gate".into());
                s.insert("voice_freq".into());
                s
            },
            declarations: vec![DeclarationScope {
                namespace: "audio".into(),
                declarations: vec![
                    NodeDeclaration {
                        node_type: "fm_osc".into(),
                        alias: Some("osc_inst".into()),
                        params: Some(object! { "freq" => Value::Template("$freq".into()) }),
                        pipes: vec![],
                    },
                    NodeDeclaration {
                        node_type: "adsr".into(),
                        alias: Some("env".into()),
                        params: Some(object! { "attack" => Value::Template("$attack".into()) }),
                        pipes: vec![],
                    },
                ],
            }],
            connections: vec![
                Connection {
                    source: Endpoint { node: "voice_freq".into(), port: Port::None },
                    sink: Endpoint { node: "osc_inst".into(), port: Port::Named("freq_in".into()) },
                },
                Connection {
                    source: Endpoint { node: "gate".into(), port: Port::None },
                    sink: Endpoint { node: "env".into(), port: Port::Named("gate".into()) },
                },
                Connection {
                    source: Endpoint { node: "osc_inst".into(), port: Port::None },
                    sink: Endpoint { node: "env".into(), port: Port::Index(1) },
                },
            ],
            sink: "env".into(),
        };

        let ast = Ast {
            macros: vec![fm_osc_macro, voice_macro],
            source: None,
            declarations: vec![
                DeclarationScope {
                    namespace: "audio".into(),
                    declarations: vec![NodeDeclaration {
                        node_type: "voice".into(),
                        alias: Some("lead".into()),
                        params: Some(object! { "freq" => 880.0f32, "attack" => 200.0f32 }),
                        pipes: vec![],
                    }],
                },
                DeclarationScope {
                    namespace: "midi".into(),
                    declarations: vec![NodeDeclaration {
                        node_type: "poly_voice".into(),
                        alias: Some("poly".into()),
                        params: None,
                        pipes: vec![],
                    }],
                },
            ],
            connections: vec![
                // External: poly.freq >> lead.voice_freq (should resolve to lead.osc_inst.carrier.freq)
                Connection {
                    source: Endpoint { node: "poly".into(), port: Port::Named("freq".into()) },
                    sink: Endpoint { node: "lead".into(), port: Port::Named("voice_freq".into()) },
                },
                // External: poly.gate >> lead.gate (should resolve to lead.env.gate)
                Connection {
                    source: Endpoint { node: "poly".into(), port: Port::Named("gate".into()) },
                    sink: Endpoint { node: "lead".into(), port: Port::Named("gate".into()) },
                },
            ],
            sink: "lead".into(),
        };

        let ir = IR::from(ast);
        dbg!(&ir);

        // --- Leaf FQNs ---
        let all_aliases: Vec<&str> = ir.declarations.iter()
            .flat_map(|s| s.declarations.iter())
            .filter_map(|d| d.alias.as_deref())
            .collect();

        assert!(all_aliases.contains(&"lead.osc_inst.modulator"), "missing lead.osc_inst.modulator");
        assert!(all_aliases.contains(&"lead.osc_inst.carrier"),   "missing lead.osc_inst.carrier");
        assert!(all_aliases.contains(&"lead.env"),                "missing lead.env");

        // --- Param propagation ---
        // lead.osc_inst.carrier should have f=880.0 (passed through two levels of templates)
        let carrier = ir.declarations.iter()
            .flat_map(|s| s.declarations.iter())
            .find(|d| d.alias.as_deref() == Some("lead.osc_inst.carrier"))
            .expect("lead.osc_inst.carrier not found");

        assert_eq!(
            carrier.params.as_ref().unwrap().get("freq"),
            Some(&Value::F32(880.0)),
            "freq template should have propagated to carrier"
        );

        let env = ir.declarations.iter()
            .flat_map(|s| s.declarations.iter())
            .find(|d| d.alias.as_deref() == Some("lead.env"))
            .expect("lead.env not found");

        assert_eq!(
            env.params.as_ref().unwrap().get("attack"),
            Some(&Value::F32(200.0)),
            "attack template should have propagated to env"
        );

        // --- Interior connections ---
        // fm_osc interior: modulator -> carrier[0]
        let mod_to_carrier = ir.connections.iter()
            .find(|c| c.source.node == "lead.osc_inst.modulator")
            .expect("modulator -> carrier connection not found");

        assert_eq!(mod_to_carrier.sink.node, "lead.osc_inst.carrier");
        assert_eq!(mod_to_carrier.sink.port, Port::Index(0));

        // voice interior: osc_inst (sink=carrier) -> env[1]
        let osc_to_env = ir.connections.iter()
            .find(|c| c.source.node == "lead.osc_inst.carrier" && c.sink.node == "lead.env")
            .expect("osc_inst -> env connection not found");

        assert_eq!(osc_to_env.sink.port, Port::Index(1));

        // --- External connections resolved through two levels of virtual ports ---
        // poly.freq >> lead.voice_freq should resolve to lead.osc_inst.carrier with port Named("freq")
        let freq_conn = ir.connections.iter()
            .find(|c| c.source.node == "poly" && c.source.port == Port::Named("freq".into()))
            .expect("freq external connection not found");

        assert_eq!(freq_conn.sink.node, "lead.osc_inst.carrier");
        assert_eq!(freq_conn.sink.port, Port::Named("freq".into()));

        // poly.gate >> lead.gate should resolve to lead.env with port Named("gate")
        let gate_conn = ir.connections.iter()
            .find(|c| c.source.node == "poly" && c.source.port == Port::Named("gate".into()))
            .expect("gate external connection not found");

        assert_eq!(gate_conn.sink.node, "lead.env");
        assert_eq!(gate_conn.sink.port, Port::Named("gate".into()));

        // --- Total connection count ---
        // 1 fm_osc interior (mod->carrier)
        // 1 voice interior (carrier->env)
        // 2 external (poly.freq->carrier, poly.gate->env)
        assert_eq!(ir.connections.len(), 4);
    }
}