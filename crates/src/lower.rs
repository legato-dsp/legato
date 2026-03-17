use crate::ir::*;
use std::collections::HashMap;

// TODO: WIP likely better data structures could be used

const MAXIMUM_DEPTH: u8 = 16;

#[derive(Default, Debug)]
pub struct Lowerer {
    // Name to the underlying macro
    registry: HashMap<String, Macro>,
}

impl Lowerer {
    pub fn lower(&mut self, ast: Ast) -> IR {
        let mut ir_decls = Vec::new();
        let mut ir_conns = Vec::new();

        for item in ast.macros {
            self.registry.insert(item.name.clone(), item);
        }

        // Loop through all of our scope declarations.
        // If any are macros, we need to expand these, and fill the connections.

        for scope in ast.declarations {
            let mut scope_decls = Vec::new();
            for decl in scope.declarations {
                if let Some(m) = self.registry.get(&decl.node_type) {
                    let fqn = format!(
                        "{}::{}",
                        &decl.alias.clone().unwrap_or_else(|| decl.node_type.clone()),
                        m.name,
                    );
                    self.expand_macro(
                        m.clone(),
                        &fqn,
                        &decl.params.clone().unwrap_or_default(),
                        &mut ir_decls,
                        &mut ir_conns,
                        0,
                    );
                } else {
                    scope_decls.push(decl);
                }
            }
            if !scope_decls.is_empty() {
                ir_decls.push(DeclarationScope {
                    namespace: scope.namespace,
                    declarations: scope_decls,
                });
            }
        }

        IR {
            declarations: ir_decls,
            connections: ir_conns,
            sink: ast.sink,
        }
    }

    fn expand_macro(
        &mut self,
        m: Macro,
        working_fqn: &str,
        params: &Object,
        declarations: &mut Vec<DeclarationScope>,
        connections: &mut Vec<Connection>,
        depth: u8,
    ) {
        if depth > MAXIMUM_DEPTH {
            panic!("Max macro depth exceeded");
        }

        // Start with default params, then add those passed in
        let mut current_params = m.default_params.clone().unwrap_or_default();
        for (k, v) in params {
            current_params.insert(k.clone(), v.clone());
        }

        // Next, we expand the definitions

        for scope in &m.declarations {
            let mut new_scope = DeclarationScope {
                namespace: scope.namespace.clone(),
                declarations: Vec::new(),
            };

            for decl in &scope.declarations {
                // We go from left to right specificity, so macro_b::macro_a::working_name
                // we can use this to keep track of multiple rounds of expansion and repair connections
                let fq_alias = format!(
                    "{}::{}",
                    decl.alias.as_ref().unwrap_or(&decl.node_type),
                    working_fqn,
                );

                // If there is another macro, we recurse and go deeper
                if let Some(inner_macro) = self.registry.get(&decl.node_type) {
                    let mut inner_params = decl.params.clone().unwrap_or_default();
                    self.resolve_templates(&mut inner_params, &current_params);

                    self.expand_macro(
                        inner_macro.clone(),
                        &fq_alias,
                        &inner_params,
                        declarations,
                        connections,
                        depth + 1,
                    );
                } else {
                    let mut leaf = decl.clone();
                    leaf.alias = Some(fq_alias);
                    if let Some(ref mut p) = leaf.params {
                        self.resolve_templates(p, &current_params);
                    }
                    new_scope.declarations.push(leaf);
                }
            }
            if !new_scope.declarations.is_empty() {
                declarations.push(new_scope);
            }
        }

        // Next, we assemble the internal connections for this macro
        for conn in &m.connections {
            connections.push(Connection {
                source: Endpoint {
                    node: format!("{}::{}", working_fqn, conn.source.node),
                    port: conn.source.port.clone(),
                },
                sink: Endpoint {
                    node: format!("{}::{}", working_fqn, conn.sink.node),
                    port: conn.sink.port.clone(),
                },
            });
        }

        // Next, we have to patch current connections for our macro
        // TODO: Rewrite with better time and memory complexity

        for conn in connections.iter_mut() {
            if conn.sink.node == m.name {
                match &conn.sink.port {
                    // Index, we match the implicit port index
                    Port::Index(idx) => {
                        let found = &m
                            .virtual_ports_in
                            .get(*idx)
                            .expect("Invalid port index passed to macro input.");
                        conn.sink.node = format!("{}::{}", working_fqn, found);
                        conn.sink.port = Port::None; // Single virtual port for now
                    }
                    Port::Named(name) => {
                        let found = &m
                            .virtual_ports_in
                            .iter()
                            .find(|x| *x == name)
                            .expect("Invalid port name passed to macro input.");
                        conn.sink.node = format!("{}::{}", working_fqn, found);
                        conn.sink.port = Port::None; // Single virtual port for now
                    }
                    Port::Slice(_, _) => {
                        unimplemented!(
                            "Port slicing on macros not yet implemented, treat virtual inputs like single input nodes"
                        )
                    }
                    Port::None => {
                        let found = &m
                            .virtual_ports_in
                            .get(0)
                            .expect("Invalid port index passed to macro input, assumed index 0, none found.");
                        conn.sink.node = format!("{}::{}", working_fqn, found);
                        conn.sink.port = Port::None;
                    }
                };
            }
            if conn.source.node == m.name {
                conn.source.node = format!("{}::{}", working_fqn, m.sink);
            }
        }
    }

    /// This function replaces any template variables
    /// with the $-less version of the word on the lookup object
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

// #[cfg(test)]
// mod tests {
//     use super::*;
//     #[test]
//     fn basic_single_instantiation() {
//         let voice_macro = Macro {
//             name: "voice".into(),
//             default_params: Some(object! {
//                 "freq" => 440.0,
//                 "attack" => 300.0,
//                 "decay" => 600.0,
//                 "sustain" => 0.5,
//                 "release" => 800.0
//             }),
//             virtual_ports_in: vec!["gate".into(), "freq_in".into()],
//             declarations: vec![
//                 DeclarationScope {
//                     namespace: "audio".into(),
//                     declarations: vec![NodeDeclaration {
//                         alias: Some("osc_one".into()),
//                         node_type: "sine".into(),
//                         params: Some(object! {
//                             "freq" => Template("$freq".into()),
//                         }),
//                         pipes: vec![],
//                     }],
//                 },
//                 DeclarationScope {
//                     namespace: "audio".into(),
//                     declarations: vec![NodeDeclaration {
//                         alias: None,
//                         node_type: "adsr".into(),
//                         params: Some(object! {
//                             "attack" => Template("$attack".into()),
//                             "decay" => Template("$decay".into()),
//                             "sustain" => Template("$sustain".into()),
//                             "release" => Template("$release".into())
//                         }),
//                         pipes: vec![],
//                     }],
//                 },
//             ],
//             connections: vec![Connection {
//                 source: Endpoint {
//                     node: "osc_one".into(),
//                     port: Port::None,
//                 },
//                 sink: Endpoint {
//                     node: "adsr".into(),
//                     port: Port::Named("gate".into()),
//                 },
//             }],
//             sink: "adsr".into(),
//         };

//         let ast = Ast {
//             macros: vec![voice_macro],
//             declarations: vec![DeclarationScope {
//                 namespace: "user".into(),
//                 declarations: vec![NodeDeclaration {
//                     node_type: "voice".into(),
//                     alias: Some("lead".into()),
//                     params: Some(object! { "freq" => 880.0 }),
//                     ..Default::default()
//                 }],
//             }],
//             ..Default::default()
//         };

//         let mut lowerer = Lowerer::default();
//         let lowered: IR = lowerer.lower(ast);

//         dbg!(&lowered);
//     }
// }

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn test_nested_expansion_with_shadowing() {
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

        // Check Alias Nesting: lead -> osc_inst -> internal_sine
        let target_node = &ir.declarations[0].declarations[0];
        assert_eq!(
            target_node.alias,
            Some("lead.osc_inst.internal_sine".into())
        );

        // Check Param Propagation: 880.0 should have reached the leaf
        let freq = target_node.params.as_ref().unwrap().get("f").unwrap();
        assert_eq!(freq, &Value::F32(880.0));
    }

    #[test]
    fn test_macro_in_macro_nesting() {
        // 1. Level 2: The smallest unit
        let sine_macro = Macro {
            name: "sine_raw".into(),
            default_params: Some(BTreeMap::from([("freq".to_string(), Value::F32(440.0))])),
            declarations: vec![DeclarationScope {
                namespace: "audio".into(),
                declarations: vec![NodeDeclaration {
                    node_type: "osc".into(),
                    alias: Some("osc_0".into()),
                    params: Some(BTreeMap::from([(
                        "hz".to_string(),
                        Value::Template("$freq".into()),
                    )])),
                    pipes: vec![],
                }],
            }],
            sink: "osc_0".into(),
            ..Default::default()
        };

        // 2. Level 1: A voice that uses the sine_raw macro
        let voice_macro = Macro {
            name: "voice".into(),
            declarations: vec![DeclarationScope {
                namespace: "patch".into(),
                declarations: vec![NodeDeclaration {
                    node_type: "sine_raw".into(),
                    alias: Some("my_sine".into()),
                    params: Some(BTreeMap::from([(
                        "freq".to_string(),
                        Value::Template("$v_freq".into()),
                    )])),
                    pipes: vec![],
                }],
            }],
            sink: "my_sine".into(),
            ..Default::default()
        };

        // 3. Level 0: AST instantiation
        let ast = Ast {
            macros: vec![sine_macro, voice_macro],
            declarations: vec![DeclarationScope {
                namespace: "user".into(),
                declarations: vec![NodeDeclaration {
                    node_type: "voice".into(),
                    alias: Some("lead_synth".into()),
                    params: Some(BTreeMap::from([("v_freq".to_string(), Value::F32(880.0))])),
                    pipes: vec![],
                }],
            }],
            ..Default::default()
        };

        let ir = IR::from(ast);

        // Verify that the deep alias is correct
        // Path: lead_synth (voice) -> my_sine (sine_raw) -> osc_0 (osc)
        let final_node = &ir.declarations[0].declarations[0];
        assert_eq!(final_node.alias, Some("lead_synth.my_sine.osc_0".into()));

        // Verify that the value propagated through 2 levels of templates
        let hz_val = final_node.params.as_ref().unwrap().get("hz").unwrap();
        assert_eq!(hz_val, &Value::F32(880.0));
    }

    #[test]
    fn test_internal_connection_fuzzing() {
        // 1. A Macro with multiple nodes and internal wiring
        let dual_osc = Macro {
            name: "dual_osc".into(),
            default_params: Some(BTreeMap::from([
                ("freq_a".to_string(), Value::F32(440.0)),
                ("freq_b".to_string(), Value::F32(445.0)),
            ])),
            declarations: vec![DeclarationScope {
                namespace: "audio".into(),
                declarations: vec![
                    NodeDeclaration {
                        node_type: "sine".into(),
                        alias: Some("osc_a".into()),
                        params: Some(BTreeMap::from([(
                            "hz".to_string(),
                            Value::Template("$freq_a".into()),
                        )])),
                        pipes: vec![],
                    },
                    NodeDeclaration {
                        node_type: "sine".into(),
                        alias: Some("osc_b".into()),
                        params: Some(BTreeMap::from([(
                            "hz".to_string(),
                            Value::Template("$freq_b".into()),
                        )])),
                        pipes: vec![],
                    },
                    NodeDeclaration {
                        node_type: "sum".into(),
                        alias: Some("mixer".into()),
                        params: None,
                        pipes: vec![],
                    },
                ],
            }],
            connections: vec![
                Connection {
                    source: Endpoint {
                        node: "osc_a".into(),
                        port: Port::None,
                    },
                    sink: Endpoint {
                        node: "mixer".into(),
                        port: Port::Named("in_0".into()),
                    },
                },
                Connection {
                    source: Endpoint {
                        node: "osc_b".into(),
                        port: Port::None,
                    },
                    sink: Endpoint {
                        node: "mixer".into(),
                        port: Port::Named("in_1".into()),
                    },
                },
            ],
            sink: "mixer".into(),
            ..Default::default()
        };

        // 2. Nest that macro inside another
        let voice_macro = Macro {
            name: "voice".into(),
            declarations: vec![DeclarationScope {
                namespace: "patch".into(),
                declarations: vec![NodeDeclaration {
                    node_type: "dual_osc".into(),
                    alias: Some("oscillators".into()),
                    params: Some(BTreeMap::from([
                        ("freq_a".to_string(), Value::Template("$v_freq".into())),
                        // freq_b will use its own default (445.0)
                    ])),
                    pipes: vec![],
                }],
            }],
            sink: "oscillators".into(),
            ..Default::default()
        };

        // 3. Instantiate the voice in the AST
        let ast = Ast {
            macros: vec![dual_osc, voice_macro],
            declarations: vec![DeclarationScope {
                namespace: "user".into(),
                declarations: vec![NodeDeclaration {
                    node_type: "voice".into(),
                    alias: Some("v1".into()),
                    params: Some(BTreeMap::from([("v_freq".to_string(), Value::F32(880.0))])),
                    pipes: vec![],
                }],
            }],
            ..Default::default()
        };

        let ir = IR::from(ast);

        // --- VALIDATION ---

        // There should be 3 nodes in the final IR for this one instance
        let flattened_decls: Vec<_> = ir
            .declarations
            .iter()
            .flat_map(|s| &s.declarations)
            .collect();
        assert_eq!(flattened_decls.len(), 3);

        // Verify Namespacing
        let aliases: Vec<_> = flattened_decls
            .iter()
            .map(|d| d.alias.as_ref().unwrap())
            .collect();
        assert!(aliases.contains(&&"v1.oscillators.osc_a".to_string()));
        assert!(aliases.contains(&&"v1.oscillators.osc_b".to_string()));
        assert!(aliases.contains(&&"v1.oscillators.mixer".to_string()));

        // Verify Connections were rewritten to the full paths
        assert_eq!(ir.connections.len(), 2);
        let conn = &ir.connections[0];
        assert_eq!(conn.source.node, "v1.oscillators.osc_a");
        assert_eq!(conn.sink.node, "v1.oscillators.mixer");

        // Verify Parameter Shadowing/Defaults
        let osc_a = flattened_decls
            .iter()
            .find(|d| d.alias == Some("v1.oscillators.osc_a".into()))
            .unwrap();
        let osc_b = flattened_decls
            .iter()
            .find(|d| d.alias == Some("v1.oscillators.osc_b".into()))
            .unwrap();

        // Osc A should be the passed 880.0
        assert_eq!(
            osc_a.params.as_ref().unwrap().get("hz").unwrap(),
            &Value::F32(880.0)
        );
        // Osc B should remain the macro default 445.0
        assert_eq!(
            osc_b.params.as_ref().unwrap().get("hz").unwrap(),
            &Value::F32(445.0)
        );
    }
}
