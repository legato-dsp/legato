//! Every `.legato` file shipped in `demos/` must build, so the gallery example
//! and the community patches it showcases stay in sync with the DSL.

use std::{fs, path::PathBuf};

use legato::{
    builder::{LegatoBuilder, Unconfigured},
    config::Config,
    spec::NodeDefinition,
};

// Kernel node registered by the gallery; the demos that use it must build too.
legato_macros::include_node!("kernels/modtap4.legato", "modtap4");

#[test]
fn all_demos_build() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("demos");

    let mut checked = 0;
    for entry in fs::read_dir(&dir).expect("demos dir should exist") {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("legato") {
            continue;
        }

        let source = fs::read_to_string(&path).unwrap();
        let config = Config {
            sample_rate: 44_100,
            block_size: 256,
            channels: 2,
            rt_capacity: 0,
        };

        let result = LegatoBuilder::<Unconfigured>::new(config)
            .register_node("audio", Modtap4::spec())
            .build_dsl(&source);
        assert!(
            result.is_ok(),
            "demo {path:?} failed to build: {:?}",
            result.err()
        );
        checked += 1;
    }

    assert!(checked > 0, "no .legato demos found in {dir:?}");
}

#[test]
fn root_readme_example_builds() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../README.md");
    let Ok(readme) = fs::read_to_string(&path) else {
        return; // The published crate has no repository README.
    };
    let example = readme
        .split("## Example Patch")
        .nth(1)
        .and_then(|rest| rest.split("```").nth(1))
        .and_then(|block| block.split_once('\n'))
        .map(|(_, body)| body)
        .expect("README should have an Example Patch code block");

    let config = Config {
        sample_rate: 44_100,
        block_size: 256,
        channels: 2,
        rt_capacity: 0,
    };
    let result = LegatoBuilder::<Unconfigured>::new(config).build_dsl(example);
    assert!(
        result.is_ok(),
        "README example failed to build: {:?}",
        result.err()
    );
}
