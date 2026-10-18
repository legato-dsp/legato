use std::{
    env, fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

const CARGO_TOML: &str = include_str!("../template/Cargo.toml.template");
const MAIN_RS: &str = include_str!("../template/main.rs.template");
const GRAPH: &str = include_str!("../template/graph.legato");
const GITIGNORE: &str = "/target\n";

const USAGE: &str = "Usage: cargo legato init [PATH]";

fn main() -> ExitCode {
    // Cargo runs subcommands as `cargo-legato legato <args>`.
    let mut args = env::args().skip(1).peekable();
    if args.peek().map(String::as_str) == Some("legato") {
        args.next();
    }

    let result = match (args.next().as_deref(), args.next(), args.next()) {
        (Some("init"), path, None) => {
            let dir = path.map_or_else(|| PathBuf::from("./legato-example"), PathBuf::from);
            init(&dir)
        }
        (Some("-hx" | "--help" | "help"), ..) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        _ => Err(USAGE.to_string()),
    };

    match result {
        Ok(name) => {
            println!("Created Legato application `{name}`, start it with `cargo run --release`");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn init(dir: &Path) -> Result<String, String> {
    fs::create_dir_all(dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    let dir = dir
        .canonicalize()
        .map_err(|e| format!("could not resolve {}: {e}", dir.display()))?;

    let name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .map(package_name)
        .ok_or_else(|| format!("could not derive a package name from {}", dir.display()))?;

    let files = [
        (
            "Cargo.toml",
            CARGO_TOML
                .replace("{{name}}", &name)
                .replace("{{version}}", env!("CARGO_PKG_VERSION")),
        ),
        ("src/main.rs", MAIN_RS.to_string()),
        ("graph.legato", GRAPH.to_string()),
        (".gitignore", GITIGNORE.to_string()),
    ];

    if let Some((existing, _)) = files.iter().find(|(path, _)| dir.join(path).exists()) {
        return Err(format!("{existing} already exists in {}", dir.display()));
    }

    fs::create_dir_all(dir.join("src")).map_err(|e| e.to_string())?;
    for (path, contents) in files {
        fs::write(dir.join(path), contents).map_err(|e| format!("could not write {path}: {e}"))?;
    }

    Ok(name)
}

fn package_name(dir_name: &str) -> String {
    let name: String = dir_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if name.starts_with(|c: char| c.is_ascii_digit()) {
        format!("app-{name}")
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir(test: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("cargo-legato-{test}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn init_writes_a_named_project() {
        let dir = scratch_dir("writes").join("my synth");

        assert_eq!(init(&dir).unwrap(), "my-synth");
        assert!(
            fs::read_to_string(dir.join("Cargo.toml"))
                .unwrap()
                .contains("name = \"my-synth\"")
        );
        assert!(dir.join("src/main.rs").exists());
        assert!(dir.join("graph.legato").exists());
    }

    #[test]
    fn init_refuses_an_existing_project() {
        let dir = scratch_dir("existing");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("Cargo.toml"), "").unwrap();

        assert!(
            init(&dir)
                .unwrap_err()
                .contains("Cargo.toml already exists")
        );
        assert!(!dir.join("src/main.rs").exists());
    }

    #[test]
    fn package_names_are_valid_crate_names() {
        assert_eq!(package_name("legato_app"), "legato_app");
        assert_eq!(package_name("my.synth"), "my-synth");
        assert_eq!(package_name("808"), "app-808");
    }
}
