#[cfg(test)]
mod parse_and_lower {
    use legato::{
        ir::{Port, Value, IR},
        parse::legato_parser,
    };

    fn parse_and_lower(src: &str) -> IR {
        let ast = legato_parser(src).expect("Parse failed");
        IR::from(ast)
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

        let ir = parse_and_lower(src);
        dbg!(&ir);

        // Correct leaves emitted
        let aliases: Vec<&str> = ir.declarations.iter()
            .flat_map(|s| s.declarations.iter())
            .filter_map(|d| d.alias.as_deref())
            .collect();

        assert!(aliases.contains(&"v1.osc"), "missing v1.osc");
        assert!(aliases.contains(&"v1.env"), "missing v1.env");

        // Params propagated from call site
        let osc = ir.declarations.iter()
            .flat_map(|s| s.declarations.iter())
            .find(|d| d.alias.as_deref() == Some("v1.osc"))
            .expect("v1.osc not found");

        assert_eq!(
            osc.params.as_ref().unwrap().get("freq"),
            Some(&Value::F32(880.0)),
            "freq should be 880.0 from call site"
        );

        let env = ir.declarations.iter()
            .flat_map(|s| s.declarations.iter())
            .find(|d| d.alias.as_deref() == Some("v1.env"))
            .expect("v1.env not found");

        assert_eq!(
            env.params.as_ref().unwrap().get("attack"),
            Some(&Value::F32(200.0)),
        );
        assert_eq!(
            env.params.as_ref().unwrap().get("release"),
            Some(&Value::F32(300.0)),
        );

        // the virtual port connections are used to build these new internal connections
        assert_eq!(ir.connections.len(), 1);
        let conn = &ir.connections[0];
        assert_eq!(conn.source.node, "v1.osc");
        assert_eq!(conn.sink.node, "v1.env");
        assert_eq!(conn.sink.port, Port::Index(1));

        assert_eq!(ir.sink, "v1");
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

        let ir = parse_and_lower(src);
        dbg!(&ir);

        // Interior + 2 external
        assert_eq!(ir.connections.len(), 3);

        let freq_conn = ir.connections.iter()
            .find(|c| c.source.node == "poly_voice"
                && c.source.port == Port::Named("freq".into()))
            .expect("freq connection not found");

        assert_eq!(freq_conn.sink.node, "v1.osc");
        assert_eq!(freq_conn.sink.port, Port::Named("freq".into()));

        let gate_conn = ir.connections.iter()
            .find(|c| c.source.node == "poly_voice"
                && c.source.port == Port::Named("gate".into()))
            .expect("gate connection not found");

        assert_eq!(gate_conn.sink.node, "v1.env");
        assert_eq!(gate_conn.sink.port, Port::Named("gate".into()));
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

        let ir = parse_and_lower(src);
        dbg!(&ir);

        let get_freq = |alias: &str| -> f32 {
            ir.declarations.iter()
                .flat_map(|s| s.declarations.iter())
                .find(|d| d.alias.as_deref() == Some(alias))
                .unwrap_or_else(|| panic!("{} not found", alias))
                .params.as_ref().unwrap()
                .get("freq")
                .and_then(|v| if let Value::F32(f) = v { Some(*f) } else { None })
                .unwrap_or_else(|| panic!("no freq on {}", alias))
        };

        assert_eq!(get_freq("v1.osc"), 110.0);
        assert_eq!(get_freq("v2.osc"), 220.0);
        assert_eq!(get_freq("v3.osc"), 440.0);
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

        let ir = parse_and_lower(src);
        dbg!(&ir);

        let aliases: Vec<&str> = ir.declarations.iter()
            .flat_map(|s| s.declarations.iter())
            .filter_map(|d| d.alias.as_deref())
            .collect();

        // Should have four nodes
        assert_eq!(aliases.len(), 4);

        assert!(aliases.contains(&"lead.osc_inst.modulator"));
        assert!(aliases.contains(&"lead.osc_inst.carrier"));
        assert!(aliases.contains(&"lead.env"));

        // Param propagated through two template levels
        let carrier = ir.declarations.iter()
            .flat_map(|s| s.declarations.iter())
            .find(|d| d.alias.as_deref() == Some("lead.osc_inst.carrier"))
            .expect("lead.osc_inst.carrier not found");

        assert_eq!(
            carrier.params.as_ref().unwrap().get("freq"),
            Some(&Value::F32(880.0))
        );

        // interior connections
        let mod_to_carrier = ir.connections.iter()
            .find(|c| c.source.node == "lead.osc_inst.modulator")
            .expect("modulator -> carrier not found");
        assert_eq!(mod_to_carrier.sink.node, "lead.osc_inst.carrier");
        assert_eq!(mod_to_carrier.sink.port, Port::Index(0));

        let osc_to_env = ir.connections.iter()
            .find(|c| c.source.node == "lead.osc_inst.carrier"
                && c.sink.node == "lead.env")
            .expect("osc_inst -> env not found");
        assert_eq!(osc_to_env.sink.port, Port::Index(1));

        // double virtual port resolution:
        // poly_voice.freq >> lead.voice_freq → lead.osc_inst.carrier Named("freq")
        let freq_conn = ir.connections.iter()
            .find(|c| c.source.node == "poly_voice"
                && c.source.port == Port::Named("freq".into()))
            .expect("freq external connection not found");

        assert_eq!(freq_conn.sink.node, "lead.osc_inst.carrier");
        assert_eq!(freq_conn.sink.port, Port::Named("freq".into()));

        // poly_voice.gate >> lead.gate → lead.env Named("gate")
        let gate_conn = ir.connections.iter()
            .find(|c| c.source.node == "poly_voice"
                && c.source.port == Port::Named("gate".into()))
            .expect("gate external connection not found");

        assert_eq!(gate_conn.sink.node, "lead.env");
        assert_eq!(gate_conn.sink.port, Port::Named("gate".into()));

        // Total: 1 fm_osc interior + 1 voice interior + 2 external
        assert_eq!(ir.connections.len(), 4);
    }

    #[test]
    fn test_e2e_passthrough_via_sink_no_virtual_port() {
        // Connecting a patch with Port::None should wire from its sink
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

        let ir = parse_and_lower(src);
        dbg!(&ir);

        let passthrough = ir.connections.iter()
            .find(|c| c.sink.node == "mixer")
            .expect("passthrough to mixer not found");

        // Should originate from the sink leaf, not the alias
        assert_eq!(passthrough.source.node, "v1.env");
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
                osc_unit: a { freq: 880.0 }, // overrides freq, gain stays default
                osc_unit: b {}               // both default
            }

            { a }
        "#;

        let ir = parse_and_lower(src);
        dbg!(&ir);

        let get_param = |alias: &str, key: &str| -> Value {
            ir.declarations.iter()
                .flat_map(|s| s.declarations.iter())
                .find(|d| d.alias.as_deref() == Some(alias))
                .unwrap_or_else(|| panic!("{} not found", alias))
                .params.as_ref().unwrap()
                .get(key)
                .cloned()
                .unwrap_or_else(|| panic!("no {} on {}", key, alias))
        };

        // a -> freq overridden
        assert_eq!(get_param("a.sine", "freq"), Value::F32(880.0));

        // b -> default freq 220.0
        assert_eq!(get_param("b.sine", "freq"), Value::F32(220.0));
    }
}