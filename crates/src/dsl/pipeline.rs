use crate::dsl::{
    expand::MacroExpansionPass, ir::*, lower::ast_to_graph, resolve::ResolvePass,
    spawn::SpawnKNodesPass,
};

/// A single, named transformation of an [`IRGraph`].
pub trait GraphPass {
    fn name(&self) -> &'static str;
    fn run(&self, graph: IRGraph) -> IRGraph;
}

/// An ordered sequence of [`GraphPass`]es applied to an [`IRGraph`].
pub struct Pipeline {
    passes: Vec<Box<dyn GraphPass>>,
}

impl Pipeline {
    pub fn new() -> Self {
        Self { passes: vec![] }
    }

    /// Append a pass to the end of the pipeline.
    pub fn add_pass<P: GraphPass + 'static>(mut self, pass: P) -> Self {
        self.passes.push(Box::new(pass));
        self
    }

    /// Translate `ast` to a literal [`IRGraph`] (see [`ast_to_graph`]), then
    /// run all passes in order.
    pub fn run_from_ast(self, ast: Ast) -> IRGraph {
        let initial = ast_to_graph(ast);
        self.run(initial)
    }

    /// Run all passes on an already-constructed graph.
    pub fn run(self, graph: IRGraph) -> IRGraph {
        self.passes.into_iter().fold(graph, |g, pass| pass.run(g))
    }
}

impl Default for Pipeline {
    /// The default pipeline. This will eventually handle sample rates, spawning nodes N times, etc.
    fn default() -> Self {
        Self::new()
            .add_pass(MacroExpansionPass)
            .add_pass(SpawnKNodesPass)
            .add_pass(ResolvePass)
    }
}

#[cfg(test)]
mod grain_example_tests {
    //! End-to-end wiring tests for the `examples/grain.rs` patch.
    //!
    //! These parse the DSL source verbatim, run the default pipeline, and assert
    //! the *exact* set of edges the source is expected to resolve to. They exist
    //! to pin down the two connection shapes that are easy to get wrong when
    //! reading the source:
    //!
    //! 1. A single, multi-channel node fanning into a port *slice* of another
    //!    node (`grain >> adsr[1..3]`), authored *inside* a macro that is then
    //!    spawned `* 3`.
    //! 2. A macro's virtual input port that fans to several interior targets
    //!    (`gate` -> both `grain.trig` and `adsr.gate`), fed from a strided port
    //!    of an upstream node (`poly_voice[0:10:3] >> voice(*).gate`).

    use super::Pipeline;
    use crate::dsl::ir::*;
    use crate::dsl::parse::legato_parser;

    /// The exact patch from `examples/grain.rs`.
    const GRAIN_PATCH: &str = r#"
        patch voice(
            attack = 50.0,
            decay = 30.0,
            sustain = 0.3,
            release = 50.0
        ) {
            in freq gate

            audio {
                grain { sampler_name: "main", chans: 2 },
                adsr { attack: $attack, decay: $decay, sustain: $sustain, release: $release, chans: 2 },
            }

            freq >> grain.freq
            gate >> grain.trig

            gate >> adsr.gate
            grain >> adsr[1..3]

            { adsr }
        }

        patches {
            voice * 3 { },
        }

        audio {
            track_mixer { tracks: 3, chans_per_track: 2 },
        }

        midi {
            poly_voice { chan: 0, voices: 3 }
        }

        poly_voice[0:10:3] >> voice(*).gate
        poly_voice[1:10:3] >> voice(*).freq

        voice(*)[0] >> track_mixer[0:6:2]
        voice(*)[1] >> track_mixer[1:6:2]

        { track_mixer }
    "#;

    fn expand(src: &str) -> IRGraph {
        let ast = legato_parser(src).expect("grain patch should parse");
        Pipeline::default().run_from_ast(ast)
    }

    /// Assert there is exactly one edge `src.src_port -> snk.snk_port`.
    fn assert_edge(graph: &IRGraph, src: &str, src_port: Port, snk: &str, snk_port: Port) {
        let matches: Vec<_> = graph
            .find_edges_between(src, snk)
            .into_iter()
            .filter(|e| e.source_port == src_port && e.sink_port == snk_port)
            .collect();
        assert_eq!(
            matches.len(),
            1,
            "expected exactly one edge {src} {src_port:?} -> {snk} {snk_port:?}, found {}\ngraph:\n{graph}",
            matches.len(),
        );
    }

    #[test]
    fn grain_patch_resolves_to_expected_wiring() {
        let graph = expand(GRAIN_PATCH);

        // The macro `voice` is spawned x3 and fully flattened: 3 grains, 3 adsrs,
        // one poly_voice, one track_mixer.
        assert_eq!(graph.node_count(), 8, "graph:\n{graph}");
        for i in 0..3 {
            assert!(
                graph
                    .find_node_by_alias(&format!("voice.{i}.grain"))
                    .is_some(),
                "missing voice.{i}.grain"
            );
            assert!(
                graph
                    .find_node_by_alias(&format!("voice.{i}.adsr"))
                    .is_some(),
                "missing voice.{i}.adsr"
            );
        }

        // poly_voice lays out [gate, freq, vel] per voice, so voice `i` reads its
        // gate from output 3*i and its freq from 3*i + 1. `gate` is a virtual port
        // that fans to *both* grain.trig and adsr.gate.
        for i in 0..3 {
            let grain = format!("voice.{i}.grain");
            let adsr = format!("voice.{i}.adsr");

            assert_edge(
                &graph,
                "poly_voice",
                Port::Index(3 * i),
                &grain,
                Port::Named("trig".into()),
            );
            assert_edge(
                &graph,
                "poly_voice",
                Port::Index(3 * i),
                &adsr,
                Port::Named("gate".into()),
            );
            assert_edge(
                &graph,
                "poly_voice",
                Port::Index(3 * i + 1),
                &grain,
                Port::Named("freq".into()),
            );

            // The interior stereo hop: grain's two outputs (Port::None = all
            // outputs) feed adsr's two signal inputs, ports [1, 2] (input 0 is
            // the named `gate`). The slice is preserved as a single edge; the
            // builder fans it 2->2 at connect time.
            assert_edge(&graph, &grain, Port::None, &adsr, Port::Slice(1, 3));
        }

        // Each voice is stereo. `voice(*)[0]` is output 0 of every voice's sink
        // (adsr), strided into the even mixer inputs; `[1]` into the odd inputs.
        for i in 0..3 {
            let adsr = format!("voice.{i}.adsr");
            assert_edge(
                &graph,
                &adsr,
                Port::Index(0),
                "track_mixer",
                Port::Index(2 * i),
            );
            assert_edge(
                &graph,
                &adsr,
                Port::Index(1),
                "track_mixer",
                Port::Index(2 * i + 1),
            );
        }

        // No stray edges: 3 voices x (trig + gate + freq + interior) + 6 mixer = 18.
        assert_eq!(
            graph.edge_count(),
            18,
            "unexpected extra edges\ngraph:\n{graph}"
        );
    }

    #[test]
    fn interior_slice_survives_macro_spawn_as_single_edge() {
        // Regression test for grain patch, we want to make sure we end up with individual edges after expansion
        let graph = expand(GRAIN_PATCH);

        for i in 0..3 {
            let grain = format!("voice.{i}.grain");
            let edges = graph.find_edges_from(&grain);
            let to_adsr: Vec<_> = edges
                .iter()
                .filter(|e| e.sink_port == Port::Slice(1, 3))
                .collect();
            assert_eq!(
                to_adsr.len(),
                1,
                "voice.{i}.grain should have exactly one sliced edge into its adsr, found {}",
                to_adsr.len(),
            );
            assert_eq!(to_adsr[0].source_port, Port::None);
        }
    }
}
