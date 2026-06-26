#[cfg(test)]
mod parse_and_lower {
    use legato::{
        dsl::ir::{IRGraph, Port, Value},
        dsl::parse::legato_parser,
        dsl::pipeline::Pipeline,
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
        assert_eq!(edges.len(), 1, "expected exactly one osc->env edge");
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

        // 1 interior (osc->env) + 2 external (poly_voice->osc, poly_voice->env)
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

        // fm_osc interior: modulator -> carrier[0]
        let mod_to_carrier =
            graph.find_edges_between("lead.osc_inst.modulator", "lead.osc_inst.carrier");
        assert_eq!(mod_to_carrier.len(), 1, "expected modulator->carrier edge");
        assert_eq!(mod_to_carrier[0].sink_port, Port::Index(0));

        // voice interior: carrier (osc_inst sink) -> env[1]
        let osc_to_env = graph.find_edges_between("lead.osc_inst.carrier", "lead.env");
        assert_eq!(osc_to_env.len(), 1, "expected carrier->env edge");
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

    #[test]
    fn test_e2e_multi_nodes() {
        let src = r#"
            patch fm(freq = 220.0) {
                audio {
                    sine: carrier { freq: $freq },
                    sine: mod { freq: $freq }
                }

                carrier >> mod.freq

                { mod }
            }

            // Fake shitty reverb patch
            patch reverb(
                chans = 2,
                gain = 1.0
            ) {
                in audio_in 

                audio {
                    allpass * 4 { delay_length: 20, feedback: 0.5, chans: $chans },
                    gain { val: $gain, chans: $chans }
                }

                audio_in >> allpass(0)
                allpass(0) >> allpass(1)
                allpass(1) >> allpass(2)
                allpass(2) >> allpass(3)
                
                // TODO: Return sink by node selector index
                allpass(3) >> gain

                { gain }
            }

            audio {
                fm * 8 { freq: 880.0 },
                track_mixer { tracks: 8, chans_per_track: 1 },
                reverb { gain: 1.0, chans: 2 }
            }

            fm(*) >> track_mixer >> reverb

            { reverb }
        "#;

        let graph = parse_and_lower(src);

        let nodes: Vec<_> = graph.topological_sort();

        assert_eq!(nodes.iter().len(), 22);

        let mixer_edges = graph.find_edges_from("track_mixer");
        assert_eq!(
            mixer_edges.len(),
            1,
            "expected exactly one edge out of track_mixer"
        );
        let allpass_0 = graph
            .find_node_by_alias("reverb.allpass.0")
            .expect("reverb.allpass.0 missing");
        assert_eq!(
            mixer_edges[0].sink, allpass_0.id,
            "track_mixer should connect to reverb.allpass.0 via the audio_in virtual port"
        );

        let reverb_gain = graph.find_node_by_alias("reverb.gain").unwrap();
        assert_eq!(
            graph.sink,
            Some(reverb_gain.id),
            "graph sink should be reverb.gain"
        );

        // 8 fm interior  (carrier -> mod.freq)  =  8
        // 3 reverb allpass chain                =  3
        // 1 reverb terminal (allpass.3 -> gain) =  1
        // 8 fm.*.mod -> track_mixer             =  8
        // 1 track_mixer -> reverb.allpass.0     =  1
        //                            = 22
        assert_eq!(graph.edge_count(), 21);
    }

    #[test]
    fn test_e2e_kitchen_sink() {
        let src = r#"
        // Level 1: leaf wrapper
        patch osc(freq = 440.0) {
            audio {
                sine { freq: $freq }
            }
            { sine }
        }

        // Level 2: three osc macros panned into a mixer
        patch triad(root = 220.0) {
            audio {
                osc: r { freq: $root },
                osc: f { freq: $root },
                osc: o { freq: $root },
                mixer { tracks: 3 }
            }

            r >> mixer[0]
            f >> mixer[1]
            o >> mixer[2]

            { mixer }
        }

        // Top level: two named triads + four spawned triads -> master gain
        patches {
            triad: chord_lo { root: 110.0 },
            triad: chord_hi { root: 880.0 },
            triad * 4       { root: 440.0 }
        }

        audio {
            gain { val: 0.5 }
        }

        chord_lo >> gain
        chord_hi >> gain
        triad(*) >> gain

        { gain }
    "#;

        let graph = parse_and_lower(src);

        // ── Node count ─────────────────────────────────────────────────────────
        // chord_lo: r.sine + f.sine + o.sine + mixer       =  4
        // chord_hi: r.sine + f.sine + o.sine + mixer       =  4
        // triad × 4: (r.sine + f.sine + o.sine + mixer) × 4 = 16
        // gain                                             =  1
        //                                                  = 25
        assert_eq!(graph.node_count(), 25);

        // ── Named instance aliases ─────────────────────────────────────────────
        for prefix in ["chord_lo", "chord_hi"] {
            for leaf in ["r.sine", "f.sine", "o.sine", "mixer"] {
                let alias = format!("{prefix}.{leaf}");
                assert!(
                    graph.find_node_by_alias(&alias).is_some(),
                    "missing {alias}"
                );
            }
        }

        // ── Spawned instance aliases ───────────────────────────────────────────
        for i in 0..4 {
            for leaf in ["r.sine", "f.sine", "o.sine", "mixer"] {
                let alias = format!("triad.{i}.{leaf}");
                assert!(
                    graph.find_node_by_alias(&alias).is_some(),
                    "missing {alias}"
                );
            }
        }

        assert!(graph.find_node_by_alias("gain").is_some(), "missing gain");

        // ── Param substitution through two macro levels ────────────────────────
        assert_eq!(
            get_param(&graph, "chord_lo.r.sine", "freq"),
            Value::F32(110.0)
        );
        assert_eq!(
            get_param(&graph, "chord_lo.f.sine", "freq"),
            Value::F32(110.0)
        );
        assert_eq!(
            get_param(&graph, "chord_hi.r.sine", "freq"),
            Value::F32(880.0)
        );
        for i in 0..4 {
            assert_eq!(
                get_param(&graph, &format!("triad.{i}.r.sine"), "freq"),
                Value::F32(440.0),
                "triad.{i}.r.sine freq wrong"
            );
        }

        // ── Interior edges (osc sinks -> mixer ports) ───────────────────────────
        // 6 named-instance interiors + 4 × 3 spawned interiors = 18
        for prefix in ["chord_lo", "chord_hi"] {
            for (osc, slot) in [("r", 0usize), ("f", 1), ("o", 2)] {
                let edges = graph.find_edges_between(
                    &format!("{prefix}.{osc}.sine"),
                    &format!("{prefix}.mixer"),
                );
                assert_eq!(
                    edges.len(),
                    1,
                    "expected {prefix}.{osc}.sine -> {prefix}.mixer"
                );
                assert_eq!(edges[0].sink_port, Port::Index(slot));
            }
        }
        for i in 0..4 {
            for (osc, slot) in [("r", 0usize), ("f", 1), ("o", 2)] {
                let edges = graph.find_edges_between(
                    &format!("triad.{i}.{osc}.sine"),
                    &format!("triad.{i}.mixer"),
                );
                assert_eq!(
                    edges.len(),
                    1,
                    "expected triad.{i}.{osc}.sine -> triad.{i}.mixer"
                );
                assert_eq!(edges[0].sink_port, Port::Index(slot));
            }
        }

        // ── Cross edges (mixer -> gain) ─────────────────────────────────────────
        // chord_lo + chord_hi + triad.0..3 = 6
        let gain_edges = graph.find_edges_to("gain");
        assert_eq!(gain_edges.len(), 6, "expected 6 edges into gain");

        let gain_id = graph.find_node_by_alias("gain").unwrap().id;
        for prefix in ["chord_lo", "chord_hi"] {
            let mixer = graph
                .find_node_by_alias(&format!("{prefix}.mixer"))
                .unwrap();
            assert!(
                gain_edges.iter().any(|e| e.source == mixer.id),
                "{prefix}.mixer -> gain edge missing"
            );
        }
        for i in 0..4 {
            let mixer = graph
                .find_node_by_alias(&format!("triad.{i}.mixer"))
                .unwrap();
            assert!(
                gain_edges.iter().any(|e| e.source == mixer.id),
                "triad.{i}.mixer -> gain edge missing"
            );
        }

        // ── Total edge count ───────────────────────────────────────────────────
        // 18 interior (3 per triad × 6 triad instances)
        //  6 cross    (each triad.mixer -> gain)
        //           = 24
        assert_eq!(graph.edge_count(), 24);

        // ── Graph sink ────────────────────────────────────────────────────────
        assert_eq!(graph.sink, Some(gain_id), "graph sink should be gain");
    }

    #[test]
    fn test_e2e_complex_ports() {
        let src = r#"
        patch channel(gain = 1.0) {
            in audio_in

            audio {
                amp { val: $gain }
            }

            audio_in >> amp

            { amp }
        }

        audio {
            sine * 6 { freq: 440.0 },
            sine: lfo { freq: 0.5 },
            channel: ch_a { gain: 0.8 },
            channel: ch_b { gain: 0.6 },
            track_mixer: mixer { tracks: 6, chans_per_track: 1 }
        }

        // Named virtual port + Index source selector:
        // only sine instance 0 feeds ch_a, only instance 1 feeds ch_b.
        sine(0) >> ch_a.audio_in
        sine(1) >> ch_b.audio_in

        // Single source broadcast to a Range of sinks via named port:
        // lfo modulates only sine.2 through sine.5, not sine.0 or sine.1.
        lfo >> sine(2..6).freq

        // Slice: range-selected sources -> contiguous indexed mixer inputs.
        // sine(2..6) gives 4 sources; mixer[0..4] gives 4 slots -> zip.
        sine(2..6) >> mixer[0..4]

        // Port::Index on outgoing macro edges:
        // ch_a and ch_b sinks wire to specific mixer slots.
        ch_a >> mixer[4]
        ch_b >> mixer[5]

        { mixer }
    "#;

        let graph = parse_and_lower(src);

        assert_eq!(graph.node_count(), 10);

        for i in 0..6 {
            assert!(
                graph.find_node_by_alias(&format!("sine.{i}")).is_some(),
                "missing sine.{i}"
            );
        }
        assert!(graph.find_node_by_alias("lfo").is_some(), "missing lfo");
        assert!(
            graph.find_node_by_alias("ch_a.amp").is_some(),
            "missing ch_a.amp"
        );
        assert!(
            graph.find_node_by_alias("ch_b.amp").is_some(),
            "missing ch_b.amp"
        );
        assert!(graph.find_node_by_alias("mixer").is_some(), "missing mixer");

        // Named virtual port + index selector
        let edges = graph.find_edges_between("sine.0", "ch_a.amp");
        assert_eq!(edges.len(), 1, "expected sine.0 -> ch_a.amp");
        assert_eq!(edges[0].source_port, Port::None);
        assert_eq!(edges[0].sink_port, Port::None);

        // sine(1) >> ch_b.audio_in resolves to sine.1 -> ch_b.amp
        let edges = graph.find_edges_between("sine.1", "ch_b.amp");
        assert_eq!(edges.len(), 1, "expected sine.1 -> ch_b.amp");

        // sine.2..5 must NOT have been accidentally routed to either channel
        for i in 2..6 {
            assert!(
                graph
                    .find_edges_between(&format!("sine.{i}"), "ch_a.amp")
                    .is_empty(),
                "sine.{i} should not connect to ch_a.amp"
            );
            assert!(
                graph
                    .find_edges_between(&format!("sine.{i}"), "ch_b.amp")
                    .is_empty(),
                "sine.{i} should not connect to ch_b.amp"
            );
        }

        // Range broadcast: lfo >> sine(2..6).freq
        for i in 2..6 {
            let edges = graph.find_edges_between("lfo", &format!("sine.{i}"));
            assert_eq!(edges.len(), 1, "expected lfo -> sine.{i}");
            assert_eq!(
                edges[0].sink_port,
                Port::Named("freq".into()),
                "lfo -> sine.{i} should target port 'freq'"
            );
        }
        // sine.0 and sine.1 are outside the range — no lfo edges
        assert!(
            graph.find_edges_between("lfo", "sine.0").is_empty(),
            "lfo should not reach sine.0"
        );
        assert!(
            graph.find_edges_between("lfo", "sine.1").is_empty(),
            "lfo should not reach sine.1"
        );

        // Slice: sine(2..6) >> mixer[0..4]
        let mixer_edges = graph.find_edges_to("mixer");
        for (slot, src_i) in (2..6usize).enumerate() {
            let src_alias = format!("sine.{src_i}");
            let src_id = graph.find_node_by_alias(&src_alias).unwrap().id;
            let edge = mixer_edges
                .iter()
                .find(|e| e.source == src_id && e.sink_port == Port::Index(slot))
                .unwrap_or_else(|| panic!("{src_alias} -> mixer[{slot}] missing"));
            assert_eq!(edge.source_port, Port::None);
        }

        // ── Port::Index on outgoing macro edges ───────────────────────────────
        // ch_a.amp -> mixer[4], ch_b.amp -> mixer[5]
        let ch_a_edges = graph.find_edges_between("ch_a.amp", "mixer");
        assert_eq!(ch_a_edges.len(), 1, "expected ch_a.amp -> mixer");
        assert_eq!(ch_a_edges[0].sink_port, Port::Index(4));

        let ch_b_edges = graph.find_edges_between("ch_b.amp", "mixer");
        assert_eq!(ch_b_edges.len(), 1, "expected ch_b.amp -> mixer");
        assert_eq!(ch_b_edges[0].sink_port, Port::Index(5));

        // ── Graph sink ────────────────────────────────────────────────────────
        let mixer_id = graph.find_node_by_alias("mixer").unwrap().id;
        assert_eq!(graph.sink, Some(mixer_id), "graph sink should be mixer");

        // ── Total edge count ──────────────────────────────────────────────────
        // 2  named virtual  (sine.0->ch_a.amp, sine.1->ch_b.amp)
        // 4  lfo broadcast  (lfo->sine.2..5 via .freq)
        // 4  slice          (sine.2..5->mixer[0..3])
        // 2  macro outgoing (ch_a.amp->mixer[4], ch_b.amp->mixer[5])
        //                  = 12
        assert_eq!(graph.edge_count(), 12);
    }

    #[test]
    fn test_e2e_stride_and_slice_port_resolution() {
        // Exercises:
        // - Port::Stride on source side of a virtual port connection,
        //   resolved to Port::Index(start + i * stride) per instance
        // - Port::Slice on outgoing macro edge, resolved to Port::Index(start + i)
        // - Port::Stride on outgoing macro edge, resolved to Port::Index(start + i * stride)
        let src = r#"
        patch voice(freq = 440.0) {
            in freq gate

            audio {
                sine { freq: $freq },
                adsr { attack: 100.0, chans: 1 }
            }

            freq >> sine.freq
            gate >> adsr.gate
            sine >> adsr[1]

            { adsr }
        }

        patches {
            voice * 5 {}
        }

        audio {
            track_mixer: osc_mixer { tracks: 5, chans_per_track: 1 },
            track_mixer: out_mixer { tracks: 2, chans_per_track: 1 }
        }

        midi {
            poly_voice { chan: 0, voices: 5 }
        }

        // Stride: poly_voice[0], [3], [6], [9], [12] → voice.0..4.gate
        // i.e. start=0, end=12, stride=3 zipped against 5 instances
        poly_voice[0:12:3] >> voice(*).gate

        // Stride: poly_voice[1], [4], [7], [10], [13] → voice.0..4.freq
        poly_voice[1:13:3] >> voice(*).freq

        // Slice: voice.0..4 adsr sinks → osc_mixer[0..5]
        voice(*) >> osc_mixer[0..5]

        // Stride on outgoing: osc_mixer sinks → out_mixer[0], [2] (stride=2)
        // (contrived but exercises the stride outgoing path)
        osc_mixer >> out_mixer

        { out_mixer }
    "#;

        let graph = parse_and_lower(src);

        // ── Node count ─────────────────────────────────────────────────────────
        // voice × 5: sine + adsr = 10
        // osc_mixer, out_mixer, poly_voice = 3
        //                                  = 13
        assert_eq!(graph.node_count(), 13);

        // ── Stride incoming: poly_voice → voice.i.adsr via gate ───────────────
        // poly_voice[0:12:3] means indices 0, 3, 6, 9, 12 for voices 0..4
        let gate_port_indices = [0usize, 3, 6, 9, 12];
        for (i, &port_index) in gate_port_indices.iter().enumerate() {
            let edges = graph.find_edges_between("poly_voice", &format!("voice.{i}.adsr"));
            let gate_edge = edges
                .iter()
                .find(|e| e.sink_port == Port::Named("gate".into()))
                .unwrap_or_else(|| panic!("poly_voice → voice.{i}.adsr gate edge missing"));
            assert_eq!(
                gate_edge.source_port,
                Port::Index(port_index),
                "poly_voice → voice.{i}.adsr should use source Port::Index({port_index})"
            );
        }

        // ── Stride incoming: poly_voice → voice.i.sine via freq ───────────────
        // poly_voice[1:13:3] means indices 1, 4, 7, 10, 13 for voices 0..4
        let freq_port_indices = [1usize, 4, 7, 10, 13];
        for (i, &port_index) in freq_port_indices.iter().enumerate() {
            let edges = graph.find_edges_between("poly_voice", &format!("voice.{i}.sine"));
            let freq_edge = edges
                .iter()
                .find(|e| e.sink_port == Port::Named("freq".into()))
                .unwrap_or_else(|| panic!("poly_voice → voice.{i}.sine freq edge missing"));
            assert_eq!(
                freq_edge.source_port,
                Port::Index(port_index),
                "poly_voice → voice.{i}.sine should use source Port::Index({port_index})"
            );
        }

        // ── No stride bleed: each instance should only receive its own index ──
        // voice.0 must NOT have source Port::Index(3), [6], [9], [12]
        for (i, &port_index) in gate_port_indices.iter().enumerate() {
            for j in 0..5usize {
                if i == j {
                    continue;
                }
                let edges = graph.find_edges_between("poly_voice", &format!("voice.{j}.adsr"));
                assert!(
                    !edges
                        .iter()
                        .any(|e| e.source_port == Port::Index(port_index)
                            && e.sink_port == Port::Named("gate".into())),
                    "Port::Index({port_index}) should only reach voice.{i}.adsr, not voice.{j}.adsr"
                );
            }
        }

        // ── Slice outgoing: voice.i.adsr → osc_mixer[i] ───────────────────────
        for i in 0..5usize {
            let edges = graph.find_edges_between(&format!("voice.{i}.adsr"), "osc_mixer");
            assert_eq!(edges.len(), 1, "expected voice.{i}.adsr → osc_mixer");
            assert_eq!(
                edges[0].sink_port,
                Port::Index(i),
                "voice.{i}.adsr should connect to osc_mixer[{i}]"
            );
        }

        // ── osc_mixer → out_mixer ─────────────────────────────────────────────
        let mixer_edges = graph.find_edges_between("osc_mixer", "out_mixer");
        assert_eq!(
            mixer_edges.len(),
            1,
            "expected exactly one osc_mixer → out_mixer edge"
        );

        // ── Graph sink ────────────────────────────────────────────────────────
        let out_mixer_id = graph.find_node_by_alias("out_mixer").unwrap().id;
        assert_eq!(
            graph.sink,
            Some(out_mixer_id),
            "graph sink should be out_mixer"
        );

        // ── Total edge count ──────────────────────────────────────────────────
        // 5  freq stride     (poly_voice → voice.i.sine)
        // 5  gate stride     (poly_voice → voice.i.adsr)
        // 5  sine → adsr[1]  (interior per voice)
        // 5  slice outgoing  (voice.i.adsr → osc_mixer[i])
        // 1  osc_mixer → out_mixer
        //                   = 21
        assert_eq!(graph.edge_count(), 21);
    }

    #[test]
    fn test_e2e_virtual_port_fans_out_to_multiple_internal_nodes() {
        // `freq` is declared as a virtual input but wired to two internal nodes:
        // freq_mult[0] and fm_add[0]. The old HashMap would silently drop one.
        let src = r#"
            patch fm_voice(freq = 440.0) {
                in freq

                audio {
                    mult: freq_mult,
                    add:  fm_add,
                    sine: carrier { freq: $freq }
                }

                freq      >> freq_mult[0]
                freq      >> fm_add[0]
                freq_mult >> carrier.freq
                fm_add    >> carrier[0]

                { carrier }
            }

            patches {
                fm_voice: v { freq: 880.0 }
            }

            audio {
                sine: lfo { freq: 2.0 }
            }

            lfo >> v.freq

            { v }
        "#;

        let graph = parse_and_lower(src);

        // lfo must reach both internal targets via the same virtual port.
        let to_freq_mult = graph.find_edges_between("lfo", "v.freq_mult");
        assert_eq!(to_freq_mult.len(), 1, "lfo -> v.freq_mult missing");
        assert_eq!(to_freq_mult[0].sink_port, Port::Index(0));

        let to_fm_add = graph.find_edges_between("lfo", "v.fm_add");
        assert_eq!(to_fm_add.len(), 1, "lfo -> v.fm_add missing");
        assert_eq!(to_fm_add[0].sink_port, Port::Index(0));

        // Interior edges are still present and correct.
        let freq_mult_to_carrier = graph.find_edges_between("v.freq_mult", "v.carrier");
        assert_eq!(freq_mult_to_carrier.len(), 1);
        assert_eq!(
            freq_mult_to_carrier[0].sink_port,
            Port::Named("freq".into())
        );

        let fm_add_to_carrier = graph.find_edges_between("v.fm_add", "v.carrier");
        assert_eq!(fm_add_to_carrier.len(), 1);
        assert_eq!(fm_add_to_carrier[0].sink_port, Port::Index(0));

        // 2 virtual fan-out + 2 interior = 4
        assert_eq!(graph.edge_count(), 4);
    }

    #[test]
    fn test_e2e_two_virtual_ports_each_fan_out_without_cross_contamination() {
        // Checking and making sure that multiple virtual inputs are mapped correctly
        let src = r#"
            patch dual(freq = 440.0, attack = 100.0) {
                in freq gate

                audio {
                    mult: freq_mult,
                    add:  fm_add,
                    adsr: env       { attack: $attack },
                    mult: env_scale
                }

                freq >> freq_mult[0]
                freq >> fm_add[0]
                gate >> env.gate
                gate >> env_scale[0]

                freq_mult >> env[1]

                { env }
            }

            patches {
                dual * 3 { freq: 220.0, attack: 50.0 }
            }

            midi {
                poly_voice { chan: 0, voices: 3 }
            }

            poly_voice[0:6:2] >> dual(*).gate
            poly_voice[1:7:2] >> dual(*).freq

            { dual }
        "#;

        let graph = parse_and_lower(src);

        // 3 instances × 4 leaf nodes = 12, plus poly_voice = 13
        assert_eq!(graph.node_count(), 13);

        for i in 0..3usize {
            let prefix = format!("dual.{i}");

            // freq fan-out: poly_voice -> freq_mult and fm_add
            let to_freq_mult =
                graph.find_edges_between("poly_voice", &format!("{prefix}.freq_mult"));
            assert_eq!(
                to_freq_mult.len(),
                1,
                "poly_voice -> {prefix}.freq_mult missing"
            );
            assert_eq!(to_freq_mult[0].sink_port, Port::Index(0));

            let to_fm_add = graph.find_edges_between("poly_voice", &format!("{prefix}.fm_add"));
            assert_eq!(to_fm_add.len(), 1, "poly_voice -> {prefix}.fm_add missing");
            assert_eq!(to_fm_add[0].sink_port, Port::Index(0));

            // gate fan-out: poly_voice -> env.gate and env_scale[0]
            let to_env = graph.find_edges_between("poly_voice", &format!("{prefix}.env"));
            let gate_edge = to_env
                .iter()
                .find(|e| e.sink_port == Port::Named("gate".into()))
                .unwrap_or_else(|| panic!("poly_voice -> {prefix}.env gate edge missing"));
            assert_eq!(gate_edge.source_port, Port::Index(i * 2)); // stride=2

            let to_env_scale =
                graph.find_edges_between("poly_voice", &format!("{prefix}.env_scale"));
            assert_eq!(
                to_env_scale.len(),
                1,
                "poly_voice -> {prefix}.env_scale missing"
            );
            assert_eq!(to_env_scale[0].sink_port, Port::Index(0));

            // Cross-contamination: freq must not reach env or env_scale
            assert!(
                graph
                    .find_edges_between("poly_voice", &format!("{prefix}.env"))
                    .iter()
                    .all(|e| e.sink_port == Port::Named("gate".into())),
                "freq port bled into {prefix}.env via wrong sink_port"
            );
            assert!(
                graph
                    .find_edges_between("poly_voice", &format!("{prefix}.freq_mult"))
                    .iter()
                    .all(|e| e.sink_port == Port::Index(0)),
                "gate port bled into {prefix}.freq_mult"
            );

            // Interior: freq_mult -> env[1]
            let interior =
                graph.find_edges_between(&format!("{prefix}.freq_mult"), &format!("{prefix}.env"));
            assert_eq!(
                interior.len(),
                1,
                "{prefix}.freq_mult -> {prefix}.env missing"
            );
            assert_eq!(interior[0].sink_port, Port::Index(1));

            // Param substitution
            assert_eq!(
                get_param(&graph, &format!("{prefix}.env"), "attack"),
                Value::F32(50.0)
            );
        }

        // 3 instances × (2 freq fan-out + 2 gate fan-out + 1 interior) = 15
        assert_eq!(graph.edge_count(), 15);
    }

    #[test]
    fn test_e2e_single_instance_macro_sink_to_port_slice() {
        // This issue was found earlier with a filter bank demo.
        // This is checking to make sure that a macro with a single instance correctly maps out.
        let src = r#"
            patch filter(freq = 3400.0, chans = 2) {
                in audio_in

                audio {
                    svf  { cutoff: $freq, chans: $chans, type: "bandpass" },
                    gain { val: 1.0, chans: $chans }
                }

                audio_in >> svf[0..2]
                svf      >> gain[0..2]

                { gain }
            }

            audio {
                filter: fb1 { freq: 80.0 },
                filter: fb2 { freq: 160.0 },
                track_mixer { chans_per_track: 2, tracks: 2 }
            }

            fb1 >> track_mixer[0..2]
            fb2 >> track_mixer[2..4]

            { track_mixer }
        "#;

        let graph = parse_and_lower(src);

        // fb1.gain -> track_mixer must keep the slice [0..2].
        let fb1_edges = graph.find_edges_between("fb1.gain", "track_mixer");
        assert_eq!(
            fb1_edges.len(),
            1,
            "expected one fb1.gain -> track_mixer edge"
        );
        assert_eq!(
            fb1_edges[0].sink_port,
            Port::Slice(0, 2),
            "single-instance macro sink should preserve the port slice, got {:?}",
            fb1_edges[0].sink_port
        );

        // fb2.gain -> track_mixer must keep the slice [2..4].
        let fb2_edges = graph.find_edges_between("fb2.gain", "track_mixer");
        assert_eq!(
            fb2_edges.len(),
            1,
            "expected one fb2.gain -> track_mixer edge"
        );
        assert_eq!(
            fb2_edges[0].sink_port,
            Port::Slice(2, 4),
            "single-instance macro sink should preserve the port slice, got {:?}",
            fb2_edges[0].sink_port
        );
    }
}

#[cfg(test)]
mod build_dsl {
    use legato::{
        builder::{LegatoBuilder, Unconfigured},
        config::Config,
        ports::PortBuilder,
    };

    pub(crate) const POLY_GRAPH: &str = r#"
        patch basic_verb(){
            in audio_in
            audio {
                svf: input_hp { type: "highpass", cutoff: 120.0, q: 0.7, chans: 2 },
                svf: input_lp { type: "lowpass", cutoff: 8000.0, q: 0.7, chans: 2 },
                svf: loop_shelf { type: "lowshelf", cutoff: 300.0, gain: -6.0, q: 0.7, chans: 8 },

                allpass: pre_ap1 { delay_length: 13.0, feedback: 0.4, chans: 2 },
                allpass: pre_ap2 { delay_length: 31.0, feedback: 0.4, chans: 2 },
                allpass: pre_ap3 { delay_length: 59.0, feedback: 0.4, chans: 2 },
                allpass: pre_ap4 { delay_length: 71.0, feedback: 0.4, chans: 2 },
                allpass: pre_ap5 { delay_length: 97.0, feedback: 0.4, chans: 2 },
                allpass: pre_ap6 { delay_length: 113.0, feedback: 0.4, chans: 2 },
                allpass: pre_ap7 { delay_length: 137.0, feedback: 0.4, chans: 2 },

                delay_write: dw1 { delay_name: "d_a", delay_length: 700.0,  chans: 2 },
                delay_write: dw2 { delay_name: "d_b", delay_length: 1000.0, chans: 2 },
                delay_write: dw3 { delay_name: "d_c", delay_length: 1500.0, chans: 2 },
                delay_write: dw4 { delay_name: "d_d", delay_length: 2500.0, chans: 2 },

                delay_read: dr1 { delay_name: "d_a", chans: 2, delay_length: 557 },
                delay_read: dr2 { delay_name: "d_b", chans: 2, delay_length: 809 },
                delay_read: dr3 { delay_name: "d_c", chans: 2, delay_length: 1201 },
                delay_read: dr4 { delay_name: "d_d", chans: 2, delay_length: 2137 },

                allpass: ap_tap1 { delay_length: 17.0, feedback: 0.4, chans: 2 },
                allpass: ap_tap2 { delay_length: 23.0, feedback: 0.4, chans: 2 },
                allpass: ap_tap3 { delay_length: 31.0, feedback: 0.4, chans: 2 },
                allpass: ap_tap4 { delay_length: 41.0, feedback: 0.4, chans: 2 },

                hadamard { chans: 8 },
                hadamard: input_had { chans: 8 },

                onepole { cutoff: 2000.0, chans: 8 },

                allpass: loop_ap1 { delay_length: 5.0,  feedback: 0.2, chans: 8 },
                allpass: loop_ap2 { delay_length: 9.0,  feedback: 0.2, chans: 8 },
                allpass: loop_ap3 { delay_length: 14.0, feedback: 0.2, chans: 8 },
                allpass: loop_ap4 { delay_length: 19.0, feedback: 0.2, chans: 8 },

                sine: lfo1 { freq: 0.11 },
                sine: lfo2 { freq: 0.13 },
                sine: lfo3 { freq: 0.17 },
                sine: lfo4 { freq: 0.19 },
                sine: lfo5 { freq: 0.07 },
                sine: lfo6 { freq: 0.23 },

                track_mixer: feedback    { tracks: 1, chans_per_track: 8, gain: [0.5] },
                track_mixer: hm_mix_down { tracks: 4, chans_per_track: 2, gain: [0.5, 0.5, 0.5, 0.5] },
                track_mixer: wet_dry     { tracks: 2, chans_per_track: 2, gain: [0.5, 0.8] },
            }

            control {
                map: lfo1_map { range: [-1.0, 1.0], new_range: [4.0,  6.0 ] },
                map: lfo2_map { range: [-1.0, 1.0], new_range: [7.5,  10.5] },
                map: lfo3_map { range: [-1.0, 1.0], new_range: [13.0, 15.0] },
                map: lfo4_map { range: [-1.0, 1.0], new_range: [17.5, 20.5] },
                map: lfo5_map { range: [-1.0, 1.0], new_range: [15.0, 19.0] },
                map: lfo6_map { range: [-1.0, 1.0], new_range: [28.0, 34.0] },
            }

            lfo1 >> lfo1_map >> loop_ap1.delay_length
            lfo2 >> lfo2_map >> loop_ap2.delay_length
            lfo3 >> lfo3_map >> loop_ap3.delay_length
            lfo4 >> lfo4_map >> loop_ap4.delay_length

            lfo5 >> lfo5_map >> ap_tap1.delay_length
            lfo6 >> lfo6_map >> ap_tap3.delay_length

            audio_in >> input_hp[0..2] >> input_lp[0..2] >> pre_ap1[0..2]
            pre_ap1[0..2] >> pre_ap2[0..2] >> pre_ap3[0..2] >> pre_ap4[0..2] >> pre_ap5[0..2] >> pre_ap6[0..2] >> pre_ap7[0..2]

            pre_ap7[0..2] >> input_had[0..2]
            pre_ap7[0..2] >> input_had[2..4]
            pre_ap7[0..2] >> input_had[4..6]
            pre_ap7[0..2] >> input_had[6..8]

            input_had[0..2] >> dw1
            input_had[2..4] >> dw2
            input_had[4..6] >> dw3
            input_had[6..8] >> dw4

            dr1 >> ap_tap1[0..2] >> hadamard[0..2]
            dr2 >> ap_tap2[0..2] >> hadamard[2..4]
            dr3 >> ap_tap3[0..2] >> hadamard[4..6]
            dr4 >> ap_tap4[0..2] >> hadamard[6..8]

            hadamard >> onepole[0..8] >> loop_shelf[0..8] >> loop_ap1[0..8] >> loop_ap2[0..8] >> loop_ap3[0..8] >> loop_ap4[0..8]

            loop_ap4    >> hm_mix_down
            hm_mix_down >> wet_dry[2..4]
            audio_in    >> wet_dry[0..2]

            loop_ap4       >> feedback
            feedback[0..2] >> dw1
            feedback[2..4] >> dw2
            feedback[4..6] >> dw3
            feedback[6..8] >> dw4

            { wet_dry }
        }

        patch voice(
            attack = 50.0,
            decay = 30.0,
            sustain = 0.3,
            release = 50.0
        ) {
            in freq gate

            audio {
                saw { chans: 1 },
                adsr { attack: $attack, decay: $decay, sustain: $sustain, release: $release, chans: 1 },
            }

            freq >> saw
            gate >> adsr.gate
            saw >> adsr[1]

            { adsr }
        }

        patches {
            voice * 5 { },
            basic_verb { }
        }

        audio {
            sine: pan_lfo { freq: 0.3 },
            pan,
            svf { chans: 2, cutoff: 5400.0, q: 0.4, type: "lowpass" },
            track_mixer: osc_mixer { tracks: 5, chans_per_track: 1, gain: [0.1, 0.1, 0.1, 0.1, 0.1] },
        }

        control {
            map { range: [-1.0, 1.0], new_range: [0.3, 0.7 ] },
        }

        midi {
            poly_voice { chan: 0, voices: 5 }
        }

        poly_voice[0:13:3] >> voice(*).gate
        poly_voice[1:13:3] >> voice(*).freq
        voice(*) >> osc_mixer[0..5]

        osc_mixer >> svf[0] >> pan[0]

        pan_lfo >> map >> pan.pan

        pan >> basic_verb

        { basic_verb }
    "#;

    #[test]
    fn test_poly_reverb_graph_builds() {
        let config = Config {
            sample_rate: 48_000,
            block_size: 1024,
            channels: 2,
            rt_capacity: 0,
        };
        let ports = PortBuilder::default().audio_out(2).build();

        // Builds the whole graph end-to-end (pipeline + builder + prepare).
        // No MIDI runtime / audio device is required to construct the app.
        let (_app, _frontend) =
            LegatoBuilder::<Unconfigured>::new(config, ports).build_dsl(POLY_GRAPH);
    }
}

#[cfg(test)]
mod build_dsl_delay {
    use legato::{
        builder::{LegatoBuilder, Unconfigured},
        config::Config,
        ports::PortBuilder,
    };

    const DELAY_GRAPH: &str = r#"
        patch filter(freq = 3400.0, chans = 2, q = 2) {
            in audio_in

            audio {
                svf  { cutoff: $freq, q: $q, chans: $chans, type: "bandpass" },
                gain { val: 1.0, chans: $chans }
            }

            audio_in >> svf[0..2]
            svf      >> gain[0..2]

            { gain }
        }

        audio {
            sampler { sampler_name: "amen" },
            filter: fb1 { freq: 80.0 },
            filter: fb2 { freq: 164.0 },
            filter: fb3 { freq: 335.0 },
            filter: fb4 { freq: 685.0, q: 3.0 },
            filter: fb5 { freq: 1402.0 },
            filter: fb6 { freq: 2868.0, q: 3.0 },
            filter: fb7 { freq: 5868.0 },
            filter: fb8 { freq: 12000.0 },
            track_mixer { chans_per_track: 2, tracks: 8 }
        }

        sampler >> fb1
        sampler >> fb2
        sampler >> fb3
        sampler >> fb4
        sampler >> fb5
        sampler >> fb6
        sampler >> fb7
        sampler >> fb8

        fb1 >> track_mixer[0..2]
        fb2 >> track_mixer[2..4]
        fb3 >> track_mixer[4..6]
        fb4 >> track_mixer[6..8]
        fb5 >> track_mixer[8..10]
        fb6 >> track_mixer[10..12]
        fb7 >> track_mixer[12..14]
        fb8 >> track_mixer[14..16]

        { track_mixer }
    "#;

    /// The build-time port-range guard must reject a connection that reads a
    /// port the node does not have (here `feedback` has only 2 outputs but the
    /// graph reads `[2..4]`). Previously this slipped through to the audio thread
    /// and panicked mid-process.
    #[test]
    #[should_panic(expected = "out of range")]
    fn test_out_of_range_port_is_rejected_at_build() {
        let config = Config {
            sample_rate: 48_000,
            block_size: 1024,
            channels: 2,
            rt_capacity: 0,
        };
        let ports = PortBuilder::default().audio_out(2).build();

        let graph = r#"
            audio {
                track_mixer: feedback { tracks: 4, chans_per_track: 2, gain: [0.5, 0.5, 0.5, 0.5] },
                delay_write: dw { delay_name: "d", delay_length: 100.0, chans: 2 }
            }

            feedback[2..4] >> dw

            { feedback }
        "#;

        let _ = LegatoBuilder::<Unconfigured>::new(config, ports).build_dsl(graph);
    }

    #[test]
    fn test_resonant_filterbank_graph_builds() {
        let config = Config {
            sample_rate: 48_000,
            block_size: 1024,
            channels: 2,
            rt_capacity: 0,
        };
        let ports = PortBuilder::default().audio_out(2).build();

        // Sample data is loaded later via the frontend; the graph builds without it.
        let (_app, _frontend) =
            LegatoBuilder::<Unconfigured>::new(config, ports).build_dsl(DELAY_GRAPH);
    }
}
