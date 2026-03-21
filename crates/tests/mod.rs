#[cfg(test)]
mod parse_and_lower {
    use legato::{
        ir::{IRGraph, Port, Value},
        lower::Pipeline,
        parse::legato_parser,
    };

    fn parse_and_lower(src: &str) -> IRGraph {
        let ast = legato_parser(src).expect("Parse failed");
        Pipeline::default().run_from_ast(ast)
    }

    /// Retrieve a param value from a node by alias, panicking with a clear
    /// message if either the node or the key is absent.
    fn get_param(graph: &IRGraph, alias: &str, key: &str) -> Value {
        graph
            .find_node_by_alias(alias)
            .unwrap_or_else(|| panic!("node '{}' not found in graph", alias))
            .params
            .get(key)
            .cloned()
            .unwrap_or_else(|| panic!("param '{}' not found on node '{}'", key, alias))
    }

    #[test]
    fn test_e2e_simple_patch_instantiation() {
        let src = r#"
            patch voice(freq = 440.0, attack = 100.0, release = 500.0) {
                in gate freq_in

                audio {
                    sine: osc { freq: $freq },
                    adsr: env { attack: $attack, release: $release }
                }

                freq_in >> osc.freq
                gate    >> env.gate
                osc     >> env[1]

                { env }
            }

            patches {
                voice: v1 { freq: 880.0, attack: 200.0, release: 300.0 }
            }

            { v1 }
        "#;

        let graph = parse_and_lower(src);

        // Both leaf nodes are present.
        assert!(
            graph.find_node_by_alias("v1.osc").is_some(),
            "missing v1.osc"
        );
        assert!(
            graph.find_node_by_alias("v1.env").is_some(),
            "missing v1.env"
        );

        // Call-site params were substituted correctly.
        assert_eq!(get_param(&graph, "v1.osc", "freq"), Value::F32(880.0));
        assert_eq!(get_param(&graph, "v1.env", "attack"), Value::F32(200.0));
        assert_eq!(get_param(&graph, "v1.env", "release"), Value::F32(300.0));

        // The two virtual-port connections (freq_in>>osc, gate>>env) produce
        // no graph edges — only the interior osc>>env[1] edge should exist.
        assert_eq!(graph.edge_count(), 1);

        let edges = graph.find_edges_between("v1.osc", "v1.env");
        assert_eq!(edges.len(), 1, "expected exactly one osc→env edge");
        assert_eq!(edges[0].sink_port, Port::Index(1));

        // Graph sink points to the voice's sink leaf (v1.env).
        let env_id = graph.find_node_by_alias("v1.env").unwrap().id;
        assert_eq!(graph.sink, Some(env_id), "graph sink should be v1.env");
    }

    #[test]
    fn test_e2e_external_connections_through_virtual_ports() {
        let src = r#"
            patch voice(freq = 440.0, attack = 100.0) {
                in gate freq_in

                audio {
                    sine: osc { freq: $freq },
                    adsr: env { attack: $attack }
                }

                freq_in >> osc.freq
                gate    >> env.gate
                osc     >> env[1]

                { env }
            }

            patches {
                voice: v1 { freq: 440.0 }
            }

            midi {
                poly_voice { chan: 0 }
            }

            poly_voice.freq >> v1.freq_in
            poly_voice.gate >> v1.gate

            { v1 }
        "#;

        let graph = parse_and_lower(src);

        // 1 interior (osc→env) + 2 external (poly_voice→osc, poly_voice→env)
        assert_eq!(graph.edge_count(), 3);

        let osc = graph.find_node_by_alias("v1.osc").expect("v1.osc missing");
        let env = graph.find_node_by_alias("v1.env").expect("v1.env missing");

        let poly_edges = graph.find_edges_from("poly_voice");

        let freq_edge = poly_edges
            .iter()
            .find(|e| e.source_port == Port::Named("freq".into()))
            .expect("poly_voice.freq edge not found");
        assert_eq!(
            freq_edge.sink, osc.id,
            "poly_voice.freq should route to v1.osc"
        );
        assert_eq!(freq_edge.sink_port, Port::Named("freq".into()));

        let gate_edge = poly_edges
            .iter()
            .find(|e| e.source_port == Port::Named("gate".into()))
            .expect("poly_voice.gate edge not found");
        assert_eq!(
            gate_edge.sink, env.id,
            "poly_voice.gate should route to v1.env"
        );
        assert_eq!(gate_edge.sink_port, Port::Named("gate".into()));
    }

    #[test]
    fn test_e2e_multiple_instances_distinct_fqns_and_params() {
        let src = r#"
            patch voice(freq = 440.0) {
                audio {
                    sine: osc { freq: $freq }
                }
                { osc }
            }

            patches {
                voice: v1 { freq: 110.0 },
                voice: v2 { freq: 220.0 },
                voice: v3 { freq: 440.0 }
            }

            { v1 }
        "#;

        let graph = parse_and_lower(src);

        // Each instance has its own leaf with its own substituted freq.
        assert_eq!(get_param(&graph, "v1.osc", "freq"), Value::F32(110.0));
        assert_eq!(get_param(&graph, "v2.osc", "freq"), Value::F32(220.0));
        assert_eq!(get_param(&graph, "v3.osc", "freq"), Value::F32(440.0));
    }

    #[test]
    fn test_e2e_nested_patch_virtual_port_resolution() {
        let src = r#"
            patch fm_osc(freq = 440.0, mod_freq = 880.0) {
                in freq_in

                audio {
                    sine: modulator { freq: $mod_freq },
                    sine: carrier   { freq: $freq }
                }

                freq_in    >> carrier.freq
                modulator  >> carrier[0]

                { carrier }
            }

            patch voice(freq = 440.0, attack = 100.0) {
                in gate voice_freq

                audio {
                    fm_osc: osc_inst { freq: $freq },
                    adsr:   env      { attack: $attack }
                }

                voice_freq >> osc_inst.freq_in
                gate       >> env.gate
                osc_inst   >> env[1]

                { env }
            }

            patches {
                voice: lead { freq: 880.0, attack: 200.0 }
            }

            midi {
                poly_voice { chan: 0 }
            }

            poly_voice.freq >> lead.voice_freq
            poly_voice.gate >> lead.gate

            { lead }
        "#;

        let graph = parse_and_lower(src);

        // Four leaf nodes total (modulator, carrier, env, poly_voice).
        assert_eq!(graph.node_count(), 4);

        for alias in [
            "lead.osc_inst.modulator",
            "lead.osc_inst.carrier",
            "lead.env",
        ] {
            assert!(
                graph.find_node_by_alias(alias).is_some(),
                "missing {}",
                alias
            );
        }

        // Param propagated through two levels of template substitution.
        assert_eq!(
            get_param(&graph, "lead.osc_inst.carrier", "freq"),
            Value::F32(880.0)
        );
        assert_eq!(get_param(&graph, "lead.env", "attack"), Value::F32(200.0));

        let carrier = graph.find_node_by_alias("lead.osc_inst.carrier").unwrap();
        let env = graph.find_node_by_alias("lead.env").unwrap();

        // fm_osc interior: modulator → carrier[0]
        let mod_to_carrier =
            graph.find_edges_between("lead.osc_inst.modulator", "lead.osc_inst.carrier");
        assert_eq!(mod_to_carrier.len(), 1, "expected modulator→carrier edge");
        assert_eq!(mod_to_carrier[0].sink_port, Port::Index(0));

        // voice interior: carrier (osc_inst sink) → env[1]
        let osc_to_env = graph.find_edges_between("lead.osc_inst.carrier", "lead.env");
        assert_eq!(osc_to_env.len(), 1, "expected carrier→env edge");
        assert_eq!(osc_to_env[0].sink_port, Port::Index(1));

        // External connections resolved through two levels of virtual ports.
        let poly_edges = graph.find_edges_from("poly_voice");

        let freq_edge = poly_edges
            .iter()
            .find(|e| e.source_port == Port::Named("freq".into()))
            .expect("poly_voice.freq edge missing");
        assert_eq!(
            freq_edge.sink, carrier.id,
            "poly_voice.freq should route to lead.osc_inst.carrier"
        );
        assert_eq!(freq_edge.sink_port, Port::Named("freq".into()));

        let gate_edge = poly_edges
            .iter()
            .find(|e| e.source_port == Port::Named("gate".into()))
            .expect("poly_voice.gate edge missing");
        assert_eq!(
            gate_edge.sink, env.id,
            "poly_voice.gate should route to lead.env"
        );
        assert_eq!(gate_edge.sink_port, Port::Named("gate".into()));

        // 1 fm_osc interior + 1 voice interior + 2 external = 4
        assert_eq!(graph.edge_count(), 4);
    }

    #[test]
    fn test_e2e_passthrough_via_sink_no_virtual_port() {
        // Connecting a patch with Port::None should originate from its sink leaf.
        let src = r#"
            patch voice(freq = 440.0) {
                audio {
                    sine: osc  { freq: $freq },
                    adsr: env  { attack: 100.0 }
                }

                osc >> env[1]

                { env }
            }

            patches {
                voice: v1 {}
            }

            audio {
                track_mixer: mixer { tracks: 1 }
            }

            v1 >> mixer

            { mixer }
        "#;

        let graph = parse_and_lower(src);

        let edges_to_mixer = graph.find_edges_to("mixer");
        assert_eq!(
            edges_to_mixer.len(),
            1,
            "expected exactly one edge into mixer"
        );

        let env = graph.find_node_by_alias("v1.env").expect("v1.env missing");
        assert_eq!(
            edges_to_mixer[0].source, env.id,
            "passthrough should originate from v1.env (the voice sink leaf), not the macro alias"
        );
    }

    #[test]
    fn test_e2e_default_params_used_when_not_overridden() {
        let src = r#"
            patch osc_unit(freq = 220.0, gain = 0.5) {
                audio {
                    sine { freq: $freq }
                }
                { sine }
            }

            patches {
                osc_unit: a { freq: 880.0 }, // overrides freq
                osc_unit: b {}               // use defaults
            }

            { a }
        "#;

        let graph = parse_and_lower(src);

        // a.sine: freq override applied.
        assert_eq!(get_param(&graph, "a.sine", "freq"), Value::F32(880.0));
        // b.sine: default
        assert_eq!(get_param(&graph, "b.sine", "freq"), Value::F32(220.0));
    }
}
