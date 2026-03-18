use crate::ir::*;
use std::collections::HashMap;

const MAXIMUM_DEPTH: u8 = 16;

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

        for scope in ast.declarations {
            for decl in scope.declarations {
                let alias = decl.alias.clone().unwrap_or_else(|| decl.node_type.clone());
                
                if let Some(m) = self.registry.get(&decl.node_type) {
                    self.expand_macro(
                        &m.clone(),
                        &alias,
                        "", // No prefix for root
                        &decl.params.clone().unwrap_or_default(),
                        &mut ir,
                        &mut scope_map,
                        0,
                    );
                } else {
                    // Normal leaf
                    let mut leaf = decl.clone();
                    leaf.alias = Some(alias);
                    scope_map.insert(scope.namespace.clone(), DeclarationScope {
                        namespace: scope.namespace.clone(),
                        declarations: vec![leaf],
                    });
                }
            }
        }

        ir.declarations = scope_map.into_values().collect();

        // TODO: Virtual ports

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
    ) -> String {
        if depth > MAXIMUM_DEPTH {
            panic!("Max macro depth exceeded at {}::{}", parent_prefix, instance_name);
        }

        // Make the FQN at this level
        let current_prefix = if parent_prefix.is_empty() {
            instance_name.to_string()
        } else {
            format!("{}.{}", parent_prefix, instance_name)
        };

        // Instantiate default params then override with those passed in
        let mut current_params = m.default_params.clone().unwrap_or_default();
        for (k, v) in params {
            current_params.insert(k.clone(), v.clone());
        }

        // Keep the symbols for this level
        let mut local_symbols = HashMap::new();

        for scope in &m.declarations {
            for decl in &scope.declarations {
                let local_alias = decl.alias.as_ref().unwrap_or(&decl.node_type).clone();

                if let Some(inner_macro) = self.registry.get(&decl.node_type) {
                    let mut inner_params = decl.params.clone().unwrap_or_default();
                    self.resolve_templates(&mut inner_params, &current_params);

                    // We can recurse here with nested templates
                    let child_sink_fqn = self.expand_macro(
                        &inner_macro.clone(),
                        &local_alias,
                        &current_prefix,
                        &inner_params,
                        ir,
                        scope_map,
                        depth + 1,
                    );
                    local_symbols.insert(local_alias, child_sink_fqn); // Push it in here so we can check for connections later
                } else {
                    // Otherwise we just have a normal node
                    let fqn = format!("{}.{}", current_prefix, local_alias);
                    let mut leaf = decl.clone();
                    leaf.alias = Some(fqn.clone());
                    
                    if let Some(ref mut p) = leaf.params {
                        self.resolve_templates(p, &current_params);
                    }

                    scope_map.entry(scope.namespace.clone())
                        .or_insert_with(|| DeclarationScope {
                            namespace: scope.namespace.clone(),
                            declarations: Vec::new(),
                        })
                        .declarations.push(leaf);

                    local_symbols.insert(local_alias, fqn); // Push it in here so we can check for connections later
                }
            }
        }

        // Build the connections for the interior of this macro
        for conn in &m.connections {
            let src_fqn = local_symbols.get(&conn.source.node)
                .expect("Source node not found in macro scope");
            let snk_fqn = local_symbols.get(&conn.sink.node)
                .expect("Sink node not found in macro scope");

            ir.connections.push(Connection {
                source: Endpoint { node: src_fqn.clone(), port: conn.source.port.clone() },
                sink: Endpoint { node: snk_fqn.clone(), port: conn.sink.port.clone() },
            });
        }

        // Return the FQN of this macro sink
        local_symbols.get(&m.sink)
            .expect("Macro sink alias does not exist in declarations")
            .clone()
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
    use super::*;

    #[test]
    fn test_single_expansion_with_connections(){
        let fm_macro = Macro {
            name: "fm".into(),
            default_params: Some(object! { "modulator_freq" => 800.0, "carrier_freq" => 400.0  }),
            declarations: vec![DeclarationScope {
                namespace: "audio".into(),
                declarations: vec![NodeDeclaration {
                    node_type: "sine".into(),
                    alias: Some("modulator".into()),
                    params: Some(object! { "f" => Value::Template("$modulator_freq".into()) }),
                    pipes: vec![],
                },
                NodeDeclaration {
                    node_type: "sine".into(),
                    alias: Some("carrier".into()),
                    params: Some(object! { "f" => Value::Template("$carrier_freq".into()) }),
                    pipes: vec![],
                }],
            }],
            connections: vec![
                Connection {
                    source: Endpoint { node: "modulator".into(), port: Port::None },
                    sink: Endpoint { node: "carrier".into(), port: Port::None }
                }
            ],
            sink: "carrier".into(),
            ..Default::default()
        };

        let ast = Ast {
            macros: vec![fm_macro],
            declarations: vec![
                DeclarationScope {
                    namespace: "audio".into(),
                    declarations: vec![
                        NodeDeclaration {
                            alias: None,
                            node_type: "fm".into(),
                            params: None,
                            pipes: vec![]
                        }
                    ]
                }
            ],
            ..Default::default()
        };

        let ir = IR::from(ast);

        dbg!(&ir);

        let modulator = &ir.declarations[0].declarations[0];
        assert_eq!(modulator.alias.as_ref().unwrap(), "fm.modulator");

        // This is faileing, it's in declarations[1] instead of adding here

        let carrier = &ir.declarations[0].declarations[1];
        assert_eq!(carrier.alias.as_ref().unwrap(), "fm.carrier");
    }

    #[test]
    fn test_basic_macro_double_expansion_no_connections() {
        let osc_macro = Macro {
            name: "osc_unit".into(),
            default_params: Some(object! { "freq" => 220.0 }),
            declarations: vec![DeclarationScope {
                namespace: "audio".into(),
                declarations: vec![NodeDeclaration {
                    node_type: "sine".into(),
                    alias: Some("internal_sine".into()),
                    params: Some(object! { "f" => Value::Template("$freq".into()) }),
                    pipes: vec![],
                }],
            }],
            sink: "internal_sine".into(),
            ..Default::default()
        };

        let voice_macro = Macro {
            name: "voice".into(),
            declarations: vec![DeclarationScope {
                namespace: "patch".into(),
                declarations: vec![NodeDeclaration {
                    node_type: "osc_unit".into(),
                    alias: Some("osc_inst".into()),
                    params: Some(object! { "freq" => Value::Template("$v_freq".into()) }),
                    pipes: vec![],
                }],
            }],
            sink: "osc_inst".into(),
            ..Default::default()
        };

        let ast = Ast {
            macros: vec![osc_macro, voice_macro],
            declarations: vec![DeclarationScope {
                namespace: "user".into(),
                declarations: vec![NodeDeclaration {
                    node_type: "voice".into(),
                    alias: Some("lead".into()),
                    params: Some(object! { "v_freq" => 880.0 }),
                    pipes: vec![],
                }],
            }],
            ..Default::default()
        };

        let ir = IR::from(ast);

        dbg!(&ir);

        let target_node = &ir.declarations[0].declarations[0];

        // 880.0 should have reached the leaf
        let freq = target_node.params.as_ref().unwrap().get("f").unwrap();
        assert_eq!(freq, &Value::F32(880.0));

        let name = target_node.alias.as_ref().unwrap();

        assert_eq!(name, "lead.osc_inst.internal_sine");
    }

    #[test]
    fn test_basic_macro_single_expansion_interior_connections() {
        let osc_macro = Macro {
            name: "fm".into(),
            default_params: Some(object! { "freq" => 220.0 }),
            declarations: vec![DeclarationScope {
                namespace: "audio".into(),
                declarations: vec![NodeDeclaration {
                    node_type: "sine".into(),
                    alias: Some("modulator".into()),
                    params: Some(object! { "f" => Value::Template("$freq".into()) }),
                    pipes: vec![],
                },
                NodeDeclaration {
                    node_type: "sine".into(),
                    alias: Some("carrier".into()),
                    params: Some(object! { "f" => Value::Template("$freq".into()) }),
                    pipes: vec![],
                }],
            }],
            sink: "carrier".into(),
            connections: vec![
                Connection {
                    source: Endpoint { node: "modulator".into(), port: Port::Index(0) },
                    sink: Endpoint { node: "carrier".into(), port: Port::Index(0) },
                } 
            ],
            ..Default::default()
        };

        let voice_macro = Macro {
            name: "voice".into(),
            declarations: vec![DeclarationScope {
                namespace: "patch".into(),
                declarations: vec![NodeDeclaration {
                    node_type: "fm".into(),
                    alias: Some("osc_inst".into()),
                    params: Some(object! { "freq" => Value::Template("$v_freq".into()) }),
                    pipes: vec![],
                }],
            }],
            sink: "osc_inst".into(),
            ..Default::default()
        };

        let ast = Ast {
            macros: vec![osc_macro, voice_macro],
            declarations: vec![DeclarationScope {
                namespace: "user".into(),
                declarations: vec![NodeDeclaration {
                    node_type: "voice".into(),
                    alias: Some("lead".into()),
                    params: Some(object! { "v_freq" => 880.0 }),
                    pipes: vec![],
                }],
            }],
            ..Default::default()
        };

        let ir = IR::from(ast);

        dbg!(&ir);

        let connection = &ir.connections[0];

        assert_eq!(connection.source.node, "lead.osc_inst.modulator");
        assert_eq!(connection.sink.node, "lead.osc_inst.carrier");
    }
}
