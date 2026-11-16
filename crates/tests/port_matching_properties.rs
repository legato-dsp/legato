//! Property tests for bare `src >> patch` name matching, checked on the built
//! graph so expansion, spawning and the builder's resolution are all covered.

use std::collections::BTreeSet;

use legato::{
    LegatoApp, LegatoFrontend,
    builder::{LegatoBuilder, Unconfigured},
    config::Config,
};
use proptest::{prelude::*, sample::subsequence};

const VOICE_OUTPUTS: [&str; 4] = ["gate", "freq", "vel", "trig"];
const FOREIGN: [&str; 4] = ["cutoff", "res", "drive", "pan"];

#[derive(Debug, Clone, Copy)]
enum Source {
    Voice,
    PolyVoice,
    /// A `constant` with this many default outputs.
    Constant(usize),
}

#[derive(Debug, Clone)]
struct Case {
    instances: usize,
    picked: (usize, usize),
    selector: String,
    source: Source,
    via_chance: bool,
    inputs: Vec<&'static str>,
}

impl Case {
    fn picked(&self) -> std::ops::Range<usize> {
        self.picked.0..self.picked.1
    }

    fn source_name(&self) -> &'static str {
        match self.source {
            Source::Voice => "voice",
            Source::PolyVoice => "poly_voice",
            Source::Constant(_) => "constant",
        }
    }

    fn source_decl(&self) -> String {
        let src = match self.source {
            Source::Voice => "voice { chan: 0 }".to_string(),
            Source::PolyVoice => {
                format!("poly_voice {{ chan: 0, voices: {} }}", self.picked().len())
            }
            Source::Constant(chans) => {
                return format!("control {{ constant {{ chans: {chans} }} }}");
            }
        };
        if self.via_chance {
            format!("midi {{ chance {{ p: 1.0 }}, {src} }}")
        } else {
            format!("midi {{ {src} }}")
        }
    }

    fn graph(&self, wiring: &str) -> String {
        let n = self.instances;
        let ins = self.inputs.join(" ");
        let body: String = self
            .inputs
            .iter()
            .enumerate()
            .map(|(j, name)| format!("{name} >> mix[{j}]\n"))
            .collect();
        let chance = if self.via_chance {
            format!("chance >> {}\n", self.source_name())
        } else {
            String::new()
        };
        format!(
            r#"
            patch synth {{
                in {ins}
                audio {{ track_mixer: mix {{ tracks: {tracks}, chans_per_track: 1 }} }}
                {body}
                {{ mix }}
            }}

            patches {{ synth * {n} {{ }} }}
            {src}
            audio {{ track_mixer: out {{ tracks: {n}, chans_per_track: 1 }} }}

            {chance}{wiring}
            synth(*) >> out[0..{n}]

            {{ out }}
            "#,
            tracks = self.inputs.len(),
            src = self.source_decl(),
        )
    }

    fn bare(&self) -> String {
        self.graph(&format!("{} >> synth{}", self.source_name(), self.selector))
    }

    fn explicit(&self, names: &[&str]) -> String {
        let wires: String = names
            .iter()
            .map(|name| {
                format!(
                    "{}.{name} >> synth{}.{name}\n",
                    self.source_name(),
                    self.selector
                )
            })
            .collect();
        self.graph(&wires)
    }

    fn mix_alias(&self, instance: usize) -> String {
        if self.instances == 1 {
            "synth.mix".to_string()
        } else {
            format!("synth.{instance}.mix")
        }
    }

    /// Source output feeding input `name` of the instance at `pos` in the selection.
    fn expected_port(&self, name: &str, pos: usize) -> usize {
        let k = VOICE_OUTPUTS.iter().position(|o| *o == name).unwrap();
        match self.source {
            Source::Voice => k,
            Source::PolyVoice => VOICE_OUTPUTS.len() * pos + k,
            Source::Constant(_) => unreachable!("a constant has no named voice outputs"),
        }
    }
}

type Wire = (String, usize, String, usize);

fn config() -> Config {
    Config {
        sample_rate: 48_000,
        block_size: 256,
        channels: 2,
        rt_capacity: 0,
    }
}

fn build(graph: &str) -> Result<(LegatoApp, LegatoFrontend), String> {
    LegatoBuilder::<Unconfigured>::new(config())
        .build_dsl(graph)
        .map_err(|e| format!("{e:?}"))
}

fn wires(app: &LegatoApp, frontend: &LegatoFrontend) -> BTreeSet<Wire> {
    let registry = frontend.node_registry();
    let alias_of: std::collections::HashMap<_, _> =
        registry.iter().map(|(a, k)| (*k, a.clone())).collect();
    registry
        .iter()
        .flat_map(|(sink, &key)| {
            let alias_of = &alias_of;
            app.incoming_connections(key).map(move |c| {
                (
                    alias_of
                        .get(&c.source.node_key)
                        .cloned()
                        .unwrap_or_else(|| "<unregistered>".to_string()),
                    c.source.port_index,
                    sink.clone(),
                    c.sink.port_index,
                )
            })
        })
        .collect()
}

fn from_source(case: &Case, wires: &BTreeSet<Wire>) -> BTreeSet<Wire> {
    wires
        .iter()
        .filter(|w| w.0 == case.source_name())
        .cloned()
        .collect()
}

fn selection() -> impl Strategy<Value = (usize, (usize, usize), String)> {
    (1usize..=5).prop_flat_map(|n| {
        let all = Just((n, (0, n), "(*)".to_string())).boxed();
        if n == 1 {
            return all;
        }
        prop_oneof![
            all,
            (0..n).prop_map(move |i| (n, (i, i + 1), format!("({i})"))),
            (0..n)
                .prop_flat_map(move |a| (Just(a), a + 1..=n))
                .prop_map(move |(a, b)| (n, (a, b), format!("({a}..{b})"))),
        ]
        .boxed()
    })
}

fn case(inputs: impl Strategy<Value = Vec<&'static str>>) -> impl Strategy<Value = Case> {
    (
        selection(),
        prop_oneof![Just(Source::Voice), Just(Source::PolyVoice)],
        any::<bool>(),
        inputs,
    )
        .prop_map(
            |((instances, picked, selector), source, via_chance, inputs)| Case {
                instances,
                picked,
                selector,
                source,
                via_chance,
                inputs,
            },
        )
}

fn constant_case(
    chans: impl Strategy<Value = usize>,
    inputs: impl Strategy<Value = Vec<&'static str>>,
) -> impl Strategy<Value = Case> {
    (selection(), chans, inputs).prop_map(|((instances, picked, selector), chans, inputs)| Case {
        instances,
        picked,
        selector,
        source: Source::Constant(chans),
        via_chance: false,
        inputs,
    })
}

fn shuffled(names: Vec<&'static str>) -> impl Strategy<Value = Vec<&'static str>> {
    Just(names).prop_shuffle()
}

fn voice_inputs() -> impl Strategy<Value = Vec<&'static str>> {
    subsequence(VOICE_OUTPUTS.to_vec(), 2..=4).prop_flat_map(shuffled)
}

fn foreign_inputs() -> impl Strategy<Value = Vec<&'static str>> {
    subsequence(FOREIGN.to_vec(), 2..=4).prop_flat_map(shuffled)
}

fn mixed_inputs() -> impl Strategy<Value = Vec<&'static str>> {
    (
        subsequence(VOICE_OUTPUTS.to_vec(), 1..=4),
        subsequence(FOREIGN.to_vec(), 1..=4),
    )
        .prop_flat_map(|(ours, foreign)| shuffled([ours, foreign].concat()))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn full_match_wires_each_input_from_its_named_output(case in case(voice_inputs())) {
        let (app, frontend) = build(&case.bare()).map_err(TestCaseError::fail)?;
        let expected: BTreeSet<Wire> = case
            .picked()
            .enumerate()
            .flat_map(|(pos, instance)| {
                let case = &case;
                case.inputs.iter().enumerate().map(move |(j, name)| {
                    (
                        case.source_name().to_string(),
                        case.expected_port(name, pos),
                        case.mix_alias(instance),
                        j,
                    )
                })
            })
            .collect();
        prop_assert_eq!(from_source(&case, &wires(&app, &frontend)), expected);
    }

    #[test]
    fn full_match_equals_explicit_named_wires(case in case(voice_inputs())) {
        let (bare_app, bare_fe) = build(&case.bare()).map_err(TestCaseError::fail)?;
        let (exp_app, exp_fe) = build(&case.explicit(&case.inputs)).map_err(TestCaseError::fail)?;
        prop_assert_eq!(wires(&bare_app, &bare_fe), wires(&exp_app, &exp_fe));
    }

    #[test]
    fn partial_match_fails_naming_both_sides(case in case(mixed_inputs())) {
        let err = build(&case.bare()).err().ok_or_else(|| {
            TestCaseError::fail("a partial name match should not build")
        })?;
        let list = |names: Vec<&str>| {
            names.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", ")
        };
        let (ours, foreign): (Vec<&str>, Vec<&str>) =
            case.inputs.iter().partition(|n| VOICE_OUTPUTS.contains(n));
        let expected = format!("matches {} but not {}", list(ours), list(foreign));
        prop_assert!(err.contains("into patch 'synth'"), "{}", err);
        prop_assert!(err.contains(&expected), "expected `{}` in {}", expected, err);
    }

    #[test]
    fn no_match_from_a_voice_fails(case in case(foreign_inputs())) {
        let err = build(&case.bare()).err().ok_or_else(|| {
            TestCaseError::fail("a voice has no default output to fall back on")
        })?;
        prop_assert!(err.contains("has no default output"), "{}", err);
    }

    #[test]
    fn no_match_from_one_default_output_feeds_the_first_input(
        case in constant_case(Just(1), foreign_inputs())
    ) {
        let (bare_app, bare_fe) = build(&case.bare()).map_err(TestCaseError::fail)?;
        let first = format!("constant >> synth{}.{}", case.selector, case.inputs[0]);
        let (exp_app, exp_fe) = build(&case.graph(&first)).map_err(TestCaseError::fail)?;
        let bare = wires(&bare_app, &bare_fe);
        prop_assert_eq!(&bare, &wires(&exp_app, &exp_fe));
        let expected: BTreeSet<Wire> = case
            .picked()
            .map(|i| ("constant".to_string(), 0, case.mix_alias(i), 0))
            .collect();
        prop_assert_eq!(from_source(&case, &bare), expected);
    }

    #[test]
    fn no_match_from_several_default_outputs_fails(
        case in constant_case(2usize..=4, foreign_inputs())
    ) {
        let err = build(&case.bare()).err().ok_or_else(|| {
            TestCaseError::fail("several default outputs cannot share one input")
        })?;
        prop_assert!(err.contains("would reduce"), "{}", err);
    }
}
