//! Emits a [`KernelPlan`] as straight-line Rust source.
//!
//! This is the codegen backend. Where [`KernelGraph`](crate::kernel::KernelGraph)
//! packs a plan into tables and walks them every sample, this walks the plan
//! *once, at build time*, and writes out a struct whose `tick` has the wiring
//! baked in: one field per node, SSA locals instead of a value table, literal
//! `+` instead of the accumulate loop, and no enum dispatch.
//!
//! Output is a `String` rather than a `TokenStream` on purpose. The near-term
//! consumer is a committed, compiled file that the equivalence test runs
//! against, and being able to *read* what was emitted is worth more right now
//! than token hygiene. `TokenStream: FromStr`, so the eventual proc macro is
//! `emit_kernel(&plan, "legato").parse()` — the only thing given up is
//! fine-grained spans, which `kernel_from_file!` was never going to have.
//!
//! # Two rules that make the output match the interpreter bit-for-bit
//!
//! 1. **Accumulators start at `0.0`.** This looks like dead weight — `0.0 + a`
//!    is `a` for every finite float — but it is load-bearing for signed zero:
//!    `0.0 + (-0.0)` is `+0.0`, while a bare `-0.0` stays negative, and the
//!    difference resurfaces as `+inf` vs `-inf` if anything downstream divides.
//!    The interpreter primes its accumulator with `0.0`, so this does too.
//! 2. **Sources are summed in plan order**, never regrouped by kind. Float
//!    addition is not associative, so reordering an interior read ahead of an
//!    exterior one silently changes the low bits, which in a feedback loop
//!    compounds into a detune. `PlanNode::inputs` is already in the
//!    interpreter's `src_pool` order; emission just follows it.
//!
//! # What is *not* duplicated here
//!
//! Generated `new()` calls [`build_kernel_node`](crate::kernel::build_kernel_node)
//! with param literals and destructures the resulting [`KernelNode`] into a
//! concrete field type. So the DSL-params-to-constructor mapping stays in one
//! place, and enum dispatch is paid once at construction rather than per
//! sample. The only thing this module knows about node types is which variant
//! holds which concrete type — a mapping whose drift is caught by
//! `every_kernel_node_type_has_a_rust_type`.

use crate::kernel_plan::{KernelPlan, PlanSrc, ValueSlot};
use std::collections::HashMap;
use std::fmt::Write as _;

/// Maps a DSL node type to the [`KernelNode`](crate::kernel::KernelNode)
/// variant that holds it and the concrete Rust type inside.
///
/// Must stay in step with `build_kernel_node`'s match arms;
/// `every_kernel_node_type_has_a_rust_type` fails if a type is added there
/// without being added here.
fn rust_type_for(node_type: &str) -> Option<(&'static str, &'static str)> {
    Some(match node_type {
        "sine" => ("Sine", "nodes::audio::sine::Sine"),
        "saw" => ("Saw", "nodes::audio::saw::Saw"),
        "svf" => ("Svf", "nodes::audio::svf::Svf"),
        "onepole" => ("OnePole", "nodes::audio::onepole::OnePole"),
        "allpass" => ("Allpass", "nodes::audio::allpass::Allpass"),
        "tap" => ("Tap", "nodes::audio::tap::DelayTap"),
        "map" => ("Map", "nodes::control::map::Map"),
        "noise" => ("Noise", "nodes::audio::noise::Noise"),
        "householder" => ("Householder", "nodes::audio::householder::HouseholderMixer"),
        "hadamard" => ("Hadamard", "nodes::audio::hadamard::HadamardMixer"),
        "pan" => ("Pan", "nodes::audio::pan::Pan"),
        // Every arithmetic node is one `ApplyOp` behind the scenes.
        "mult" | "add" | "sub" | "div" | "gain" => ("Op", "nodes::audio::ops::ApplyOp"),
        _ => return None,
    })
}

/// Rewrite a DSL alias into something usable as a Rust identifier fragment.
///
/// Kernel-body aliases are normally plain, but nothing in the grammar forbids
/// characters Rust would reject. Generated identifiers are always prefixed
/// (`n_`, `v_`, `z_`), so sanitizing is enough on its own — no keyword can be
/// produced, and no collision with the struct's own items is possible.
fn sanitize(alias: &str) -> String {
    alias
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// `fm3` -> `Fm3`, `plate_verb` -> `PlateVerb`.
fn pascal_case(name: &str) -> String {
    name.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// Render a [`Value`](crate::dsl::ir::Value) as a Rust expression building the
/// same value.
///
/// Floats use `{:?}`, which is Rust's shortest round-tripping representation —
/// the emitted literal parses back to bit-identical f32. Non-finite values get
/// named constants, since `inf` and `NaN` are not valid literals.
fn value_literal(value: &crate::dsl::ir::Value, krate: &str) -> String {
    use crate::dsl::ir::Value;

    match value {
        Value::Null => format!("{krate}::dsl::ir::Value::Null"),
        Value::U32(v) => format!("{krate}::dsl::ir::Value::U32({v})"),
        Value::I32(v) => format!("{krate}::dsl::ir::Value::I32({v})"),
        Value::F32(v) => {
            let lit = if v.is_nan() {
                "f32::NAN".to_string()
            } else if v.is_infinite() {
                if v.is_sign_positive() {
                    "f32::INFINITY".to_string()
                } else {
                    "f32::NEG_INFINITY".to_string()
                }
            } else {
                format!("{v:?}f32")
            };
            format!("{krate}::dsl::ir::Value::F32({lit})")
        }
        Value::Bool(v) => format!("{krate}::dsl::ir::Value::Bool({v})"),
        Value::Ident(v) => format!("{krate}::dsl::ir::Value::Ident({v:?}.to_string())"),
        Value::String(v) => format!("{krate}::dsl::ir::Value::String({v:?}.to_string())"),
        Value::Array(items) => {
            let items: Vec<String> = items.iter().map(|i| value_literal(i, krate)).collect();
            format!("{krate}::dsl::ir::Value::Array(vec![{}])", items.join(", "))
        }
        Value::Object(map) => {
            let mut out = String::from("{ let mut o = std::collections::BTreeMap::new(); ");
            for (k, v) in map {
                let _ = write!(
                    out,
                    "o.insert({k:?}.to_string(), {}); ",
                    value_literal(v, krate)
                );
            }
            out.push_str("o }");
            format!("{krate}::dsl::ir::Value::Object({out})")
        }
        // Templates are substituted during plan resolution, so one surviving
        // here means resolution was skipped — emit something that fails loudly
        // rather than silently baking in a placeholder.
        Value::Template(t) => {
            format!("compile_error!(\"unsubstituted template ${t} reached codegen\")")
        }
    }
}

/// Build the expression for one input port, mirroring the interpreter's
/// accumulate loop exactly. See the module docs for why order and the `0.0`
/// prime are not negotiable.
fn port_expression(sources: &[PlanSrc], slot_names: &HashMap<u32, String>) -> String {
    if sources.is_empty() {
        // Unpatched: the node falls back to its internal param.
        return "None".to_string();
    }

    let read = |src: &PlanSrc| -> Option<String> {
        match src {
            PlanSrc::Interior {
                slot: ValueSlot(s),
                delayed,
            } => {
                let name = &slot_names[s];
                Some(if *delayed {
                    // Back edge: last tick's value, held in a z field.
                    format!("self.z_{name}")
                } else {
                    format!("v_{name}")
                })
            }
            PlanSrc::Exterior(_) => None,
        }
    };

    // All-interior is the common case and is statically patched, so it needs
    // no runtime flag — just the sum, primed with 0.0.
    if sources
        .iter()
        .all(|s| matches!(s, PlanSrc::Interior { .. }))
    {
        let terms: Vec<String> = sources.iter().filter_map(read).collect();
        return format!("Some(0.0f32 + {})", terms.join(" + "));
    }

    // Mixed or exterior-only: whether the port counts as patched depends on
    // which exterior inputs are live this sample, so carry the flag.
    let mut body = String::from("{ let mut acc = 0.0f32; let mut patched = false; ");
    for src in sources {
        match src {
            PlanSrc::Interior { .. } => {
                let expr = read(src).expect("interior source");
                let _ = write!(body, "acc += {expr}; patched = true; ");
            }
            PlanSrc::Exterior(i) => {
                let _ = write!(
                    body,
                    "if let Some(v) = in_frame[{i}] {{ acc += v; patched = true; }} "
                );
            }
        }
    }
    body.push_str("if patched { Some(acc) } else { None } }");
    body
}

/// Resolve one kernel out of DSL source text and emit it as Rust.
///
/// This is the whole compile-time pipeline in one call — parse, resolve, emit —
/// and is what the `include_node!` proc macro drives. It lives here rather than
/// in the macro crate so the macro stays a thin shim and everything testable
/// stays testable without a proc-macro harness.
///
/// `krate` is the path prefix for legato items: `"legato"` for downstream
/// users, `"crate"` when the output is compiled inside legato itself.
pub fn generate_node(
    source: &str,
    kernel_name: &str,
    krate: &str,
) -> Result<String, crate::builder::ValidationError> {
    use crate::{
        builder::ValidationError,
        config::{BlockSize, Config},
        dsl::{ir::Object, lower::ast_to_graph, parse::legato_parser},
        kernel::ProbeOracle,
        kernel_plan::resolve_plan,
    };

    // The kernel is a declaration, so the parser needs a complete program.
    // A trivial patch is appended rather than requiring one in the file.
    let program = format!("{source}\n audio {{ sine }} {{ sine }}");
    let ast = legato_parser(&program).map_err(|e| ValidationError::ParseError(format!("{e:?}")))?;

    let definition = ast_to_graph(ast)
        .macro_registry
        .get(kernel_name)
        .ok_or_else(|| {
            ValidationError::NodeNotFound(format!("no kernel named '{kernel_name}' in this file"))
        })?
        .clone();

    // Sample rate is a runtime property — the generated `new()` takes it from
    // the resource builder — so any rate works for resolving topology. Port
    // arity never depends on it.
    let config = Config::new(48_000, BlockSize::Block64, 1, 0);

    // The kernel name salts identity seeds. Two generated instances of the same
    // kernel therefore share noise seeds, unlike two runtime instantiations
    // which are salted by their distinct aliases. That is a real limitation for
    // polyphony and is tied to the same param work that would let a generated
    // node be configured per instance.
    let plan = resolve_plan(
        &definition,
        &Object::new(),
        kernel_name,
        &mut ProbeOracle::new(&config),
    )?;

    Ok(emit_kernel(&plan, krate))
}

/// Emit `plan` as a Rust module body.
///
/// `krate` is the path prefix for legato items — `"crate"` when the output is
/// compiled inside legato itself, `"legato"` for downstream users.
pub fn emit_kernel(plan: &KernelPlan, krate: &str) -> String {
    let struct_name = pascal_case(&plan.name);

    // slot -> sanitized "alias_port", the stem for the v_/z_ identifiers that
    // carry that slot's value.
    let mut slot_names: HashMap<u32, String> = HashMap::new();
    for node in &plan.nodes {
        for port in 0..node.n_out {
            slot_names.insert(
                node.slot_base.0 + port as u32,
                format!("{}_{port}", sanitize(&node.alias)),
            );
        }
    }

    // Which slots are read across a back edge, and therefore need a z field.
    let mut delayed_slots: Vec<u32> = plan
        .nodes
        .iter()
        .flat_map(|n| n.inputs.iter())
        .flatten()
        .filter_map(|src| match src {
            PlanSrc::Interior {
                slot: ValueSlot(s),
                delayed: true,
            } => Some(*s),
            _ => None,
        })
        .collect();
    delayed_slots.sort_unstable();
    delayed_slots.dedup();

    let max_out = plan.nodes.iter().map(|n| n.n_out).max().unwrap_or(1).max(1);

    let mut out = String::new();
    emit_header(&mut out, plan, &struct_name, krate);
    emit_struct(
        &mut out,
        plan,
        &struct_name,
        &delayed_slots,
        &slot_names,
        krate,
    );
    emit_new(
        &mut out,
        plan,
        &struct_name,
        &delayed_slots,
        &slot_names,
        krate,
    );
    emit_tick(
        &mut out,
        plan,
        &struct_name,
        &delayed_slots,
        &slot_names,
        max_out,
        krate,
    );
    emit_node_definition(&mut out, plan, &struct_name, krate);
    out
}

/// Emit the [`NodeDefinition`](crate::spec::NodeDefinition) impl that makes a
/// generated kernel a first-class node: registerable by name and usable from a
/// block-rate `.legato` graph exactly like a built-in.
///
/// `PerSample` block-adapts the per-sample `tick` to the graph's block rate.
///
/// # Params are baked at generation time
///
/// `create`'s `params` argument is deliberately ignored: everything the kernel
/// declares is resolved when the plan is built, so `verb { decay: 0.7 }` in a
/// graph has no effect on a generated node. That is the structural-vs-runtime
/// param split, still unimplemented — until it lands, the `.legato` file's
/// declared defaults are the only way to set a generated kernel's values.
fn emit_node_definition(out: &mut String, plan: &KernelPlan, struct_name: &str, krate: &str) {
    let _ = writeln!(
        out,
        "\nimpl {krate}::spec::NodeDefinition for {struct_name} {{"
    );
    let _ = writeln!(out, "    const NAME: &'static str = {:?};", plan.name);
    let _ = writeln!(
        out,
        "    const DESCRIPTION: &'static str = {:?};",
        format!("Generated from the `{}` kernel", plan.name)
    );
    let _ = writeln!(
        out,
        "    const REQUIRED_PARAMS: &'static [&'static str] = &[];"
    );
    // Declared so a graph naming them is not rejected, even though generated
    // code cannot yet apply them. See the note above.
    let declared: Vec<String> = plan.param_names.iter().map(|n| format!("{n:?}")).collect();
    let _ = writeln!(
        out,
        "    const OPTIONAL_PARAMS: &'static [&'static str] = &[{}];",
        declared.join(", ")
    );
    let _ = writeln!(
        out,
        "\n    fn create(\n                 rb: &mut {krate}::builder::ResourceBuilderView,\n                 _params: &{krate}::dsl::ir::DSLParams,\n    )          -> Result<Box<dyn {krate}::node::DynNode>, {krate}::builder::ValidationError> {{"
    );
    let _ = writeln!(
        out,
        "        Ok(Box::new({krate}::persample::PerSample::new(Self::new(rb)?)))"
    );
    let _ = writeln!(out, "    }}");
    let _ = writeln!(out, "}}");
}

fn emit_header(out: &mut String, plan: &KernelPlan, struct_name: &str, _krate: &str) {
    let _ = write!(
        out,
        "// @generated by legato's kernel emitter from kernel `{}`. Do not edit.\n\
         //\n\
         // Regenerate rather than patching: this file is asserted to be exactly\n\
         // what `emit_kernel` produces, so hand edits will fail the snapshot test.\n\n",
        plan.name
    );
    let _ = writeln!(
        out,
        "/// The `{}` kernel, lowered to straight-line Rust.\n\
         ///\n\
         /// One field per interior node; `z_*` fields hold the previous sample for\n\
         /// reads that cross a feedback edge.\n\
         #[derive(Clone)]",
        plan.name
    );
    let _ = writeln!(out, "pub struct {struct_name} {{");
}

fn emit_struct(
    out: &mut String,
    plan: &KernelPlan,
    _struct_name: &str,
    delayed_slots: &[u32],
    slot_names: &HashMap<u32, String>,
    krate: &str,
) {
    for node in &plan.nodes {
        let (_, ty) = rust_type_for(&node.node_type)
            .unwrap_or_else(|| panic!("no Rust type mapped for node type '{}'", node.node_type));
        let _ = writeln!(out, "    n_{}: {krate}::{ty},", sanitize(&node.alias));
    }
    for slot in delayed_slots {
        let _ = writeln!(
            out,
            "    /// z⁻¹ for `{}` (read across a back edge).",
            slot_names[slot]
        );
        let _ = writeln!(out, "    z_{}: f32,", slot_names[slot]);
    }
    let _ = writeln!(out, "    ports: {krate}::ports::Ports,");
    let _ = writeln!(out, "}}\n");
}

fn emit_new(
    out: &mut String,
    plan: &KernelPlan,
    struct_name: &str,
    delayed_slots: &[u32],
    slot_names: &HashMap<u32, String>,
    krate: &str,
) {
    let _ = writeln!(out, "impl {struct_name} {{");
    let _ = writeln!(
        out,
        "    /// Build the kernel's DSP state. Sample rate and delay-line\n    \
         /// allocation both come from `rb`."
    );
    let _ = writeln!(
        out,
        "    pub fn new(rb: &mut {krate}::builder::ResourceBuilderView) \
         -> Result<Self, {krate}::builder::ValidationError> {{"
    );

    for node in &plan.nodes {
        let field = sanitize(&node.alias);
        let (variant, ty) = rust_type_for(&node.node_type).expect("checked in emit_struct");

        let _ = writeln!(out, "        let n_{field} = {{");
        let _ = writeln!(
            out,
            "            let mut params = std::collections::BTreeMap::new();"
        );
        for (key, value) in &node.params {
            let _ = writeln!(
                out,
                "            params.insert({key:?}.to_string(), {});",
                value_literal(value, krate)
            );
        }
        // Construction goes through the shared builder so param handling is
        // never reimplemented here; the enum is unwrapped immediately so the
        // hot path holds a concrete type.
        let _ = writeln!(
            out,
            "            let built = {krate}::kernel::build_kernel_node({:?}, rb, \
             &{krate}::dsl::ir::DSLParams::new(&params), {}u32)?;",
            node.node_type, node.identity_seed
        );
        let _ = writeln!(
            out,
            "            match built {{\n                \
             {krate}::kernel::KernelNode::{variant}(inner) => inner,\n                \
             _ => unreachable!(\"'{}' must build a {}\"),\n            \
             }}",
            node.node_type,
            ty.rsplit("::").next().unwrap_or(ty)
        );
        let _ = writeln!(out, "        }};");
    }

    let in_names: Vec<String> = plan.input_names.iter().map(|n| format!("{n:?}")).collect();
    let _ = writeln!(out, "\n        Ok(Self {{");
    for node in &plan.nodes {
        let field = sanitize(&node.alias);
        let _ = writeln!(out, "            n_{field},");
    }
    for slot in delayed_slots {
        let _ = writeln!(out, "            z_{}: 0.0,", slot_names[slot]);
    }
    let _ = writeln!(
        out,
        "            ports: {krate}::ports::PortBuilder::default()"
    );
    if in_names.is_empty() {
        let _ = writeln!(out, "                .audio_in(0)");
    } else {
        let _ = writeln!(
            out,
            "                .audio_in_named(&[{}])",
            in_names.join(", ")
        );
    }
    let _ = writeln!(
        out,
        "                .audio_out({})\n                .build(),",
        plan.output_slots.len()
    );
    let _ = writeln!(out, "        }})");
    let _ = writeln!(out, "    }}");
    let _ = writeln!(out, "}}\n");
}

fn emit_tick(
    out: &mut String,
    plan: &KernelPlan,
    struct_name: &str,
    delayed_slots: &[u32],
    slot_names: &HashMap<u32, String>,
    max_out: usize,
    krate: &str,
) {
    let _ = writeln!(
        out,
        "impl {krate}::persample::PerSampleNode for {struct_name} {{"
    );
    let _ = writeln!(
        out,
        "    fn ports(&self) -> &{krate}::ports::Ports {{\n        \
         &self.ports\n    }}\n"
    );
    let _ = writeln!(out, "    #[allow(unused_variables)]");
    let _ = writeln!(
        out,
        "    fn tick(&mut self, in_frame: &[Option<f32>], out_frame: &mut [f32]) {{"
    );
    let _ = writeln!(
        out,
        "        // Scratch for each node's outputs; every node owns its own state.\n        \
         let mut o = [0.0f32; {max_out}];\n"
    );

    for node in &plan.nodes {
        let field = sanitize(&node.alias);
        let args: Vec<String> = node
            .inputs
            .iter()
            .map(|srcs| port_expression(srcs, slot_names))
            .collect();

        let _ = writeln!(
            out,
            "        self.n_{field}.tick(&[{}], &mut o[..{}]);",
            args.join(", "),
            node.n_out
        );
        for port in 0..node.n_out {
            let _ = writeln!(
                out,
                "        let v_{} = o[{port}];",
                slot_names[&(node.slot_base.0 + port as u32)]
            );
        }
        out.push('\n');
    }

    for (j, ValueSlot(slot)) in plan.output_slots.iter().enumerate() {
        let _ = writeln!(out, "        out_frame[{j}] = v_{};", slot_names[slot]);
    }

    if !delayed_slots.is_empty() {
        let _ = writeln!(
            out,
            "\n        // Commit the one-sample delays for the next tick."
        );
        for slot in delayed_slots {
            let name = &slot_names[slot];
            let _ = writeln!(out, "        self.z_{name} = v_{name};");
        }
    }

    let _ = writeln!(out, "    }}");
    let _ = writeln!(out, "}}");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The emitter's node-type table must cover everything `build_kernel_node`
    /// accepts. Without this, adding a per-sample node upstream would leave
    /// codegen panicking at emit time on a kernel that the interpreter runs
    /// perfectly well.
    #[test]
    fn every_kernel_node_type_has_a_rust_type() {
        // Mirrors `build_kernel_node`'s arms.
        const KERNEL_CAPABLE: &[&str] = &[
            "sine",
            "saw",
            "svf",
            "onepole",
            "allpass",
            "tap",
            "map",
            "noise",
            "householder",
            "hadamard",
            "pan",
            "mult",
            "add",
            "sub",
            "div",
            "gain",
        ];

        let missing: Vec<&str> = KERNEL_CAPABLE
            .iter()
            .copied()
            .filter(|t| rust_type_for(t).is_none())
            .collect();

        assert!(
            missing.is_empty(),
            "node types accepted by build_kernel_node but unmapped in the emitter: {missing:?}"
        );
    }

    #[test]
    fn pascal_case_handles_separators() {
        assert_eq!(pascal_case("fm3"), "Fm3");
        assert_eq!(pascal_case("plate_verb"), "PlateVerb");
        assert_eq!(pascal_case("modtap4"), "Modtap4");
    }

    /// Signed zero is the reason accumulators are primed with `0.0` rather than
    /// starting from the first term; see the module docs.
    #[test]
    fn interior_sums_are_primed_with_zero() {
        let mut names = HashMap::new();
        names.insert(0u32, "a_0".to_string());
        names.insert(1u32, "b_0".to_string());

        let sources = vec![
            PlanSrc::Interior {
                slot: ValueSlot(0),
                delayed: false,
            },
            PlanSrc::Interior {
                slot: ValueSlot(1),
                delayed: true,
            },
        ];

        assert_eq!(
            port_expression(&sources, &names),
            "Some(0.0f32 + v_a_0 + self.z_b_0)"
        );
    }

    #[test]
    fn unpatched_port_is_none() {
        assert_eq!(port_expression(&[], &HashMap::new()), "None");
    }
}
