//! Compile-time codegen for Legato kernels.
//!
//! [`include_node!`] reads a `.legato` file at compile time, resolves the named
//! kernel to a plan, and expands to straight-line Rust implementing both
//! [`PerSampleNode`] and [`NodeDefinition`] — so a kernel written in the DSL
//! becomes a node indistinguishable from a hand-written Rust one, registerable
//! by name and usable from a block-rate graph.
//!
//! This runs Legato's own parser, resolver and emitter, so the DSL has one
//! implementation shared with the runtime interpreter rather than a second one
//! that could drift.

use proc_macro::TokenStream;

/// Generate a node from a kernel in a `.legato` file.
///
/// ```ignore
/// legato_macros::include_node!("kernels/modtap4.legato", "modtap4");
/// ```
///
/// The path is relative to the invoking crate's manifest directory. The second
/// argument names the kernel within the file.
///
/// # Params come from the file
///
/// Values are resolved at expansion time from the kernel's declared defaults,
/// so the `.legato` file is the single source of truth. Setting params on the
/// instantiation in a graph does not affect a generated node — that needs the
/// structural-vs-runtime param split, which is not implemented yet.
#[proc_macro]
pub fn include_node(input: TokenStream) -> TokenStream {
    let args = string_literals(input);
    let (path, kernel) = match args.as_slice() {
        [path, kernel] => (path, kernel),
        _ => {
            return error(
                "include_node! expects two string literals: \
                 include_node!(\"path/to/file.legato\", \"kernel_name\")",
            );
        }
    };

    let manifest_dir = match std::env::var("CARGO_MANIFEST_DIR") {
        Ok(dir) => dir,
        Err(_) => return error("CARGO_MANIFEST_DIR is not set; cannot resolve the kernel path"),
    };
    let full_path = std::path::Path::new(&manifest_dir).join(path);

    let source = match std::fs::read_to_string(&full_path) {
        Ok(source) => source,
        Err(e) => return error(&format!("could not read {}: {e}", full_path.display())),
    };

    let generated = match legato::kernel_emit::generate_node(&source, kernel, "legato") {
        Ok(generated) => generated,
        Err(e) => return error(&format!("kernel '{kernel}' in {path}: {e:?}")),
    };

    // `include_str!` makes rustc track the .legato file as an input, so editing
    // it triggers a rebuild. Without this the expansion silently goes stale
    // until something else in the crate changes — the exact class of bug that
    // only reproduces on someone else's machine. The stable alternative,
    // `proc_macro::tracked_path`, is still unstable.
    //
    // An absolute path is used because `include_str!` resolves relative to the
    // *source file* invoking it, which a proc macro cannot know.
    let tracker = format!(
        "const _: &str = include_str!({:?});\n",
        full_path.display().to_string()
    );

    match format!("{tracker}{generated}").parse() {
        Ok(tokens) => tokens,
        Err(e) => error(&format!("generated code did not parse: {e}")),
    }
}

/// Pull the string literals out of the macro input, ignoring separators.
///
/// Deliberately hand-rolled rather than pulling in `syn`: the entire grammar
/// here is "two string literals", and `syn` is a heavy compile-time dependency
/// for every downstream user of the macro.
fn string_literals(input: TokenStream) -> Vec<String> {
    input
        .into_iter()
        .filter_map(|tree| match tree {
            proc_macro::TokenTree::Literal(literal) => {
                let text = literal.to_string();
                let trimmed = text.trim();
                // Only accept plain `"..."` literals; anything else (numbers,
                // byte strings, raw strings) is not a path or kernel name.
                if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
                    Some(trimmed[1..trimmed.len() - 1].to_string())
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect()
}

fn error(message: &str) -> TokenStream {
    format!("compile_error!({message:?});")
        .parse()
        .expect("compile_error! should always parse")
}
