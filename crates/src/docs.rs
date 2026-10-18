//! Export the catalog of built-in nodes for the docs site.
//!
//! The catalog is read from the same registries the builder starts with, so it
//! cannot drift from what the DSL accepts.

use crate::registry::default_namespaces;

/// One built-in node, as listed in `docs/content/nodes.json`.
#[derive(Debug, PartialEq)]
#[cfg_attr(feature = "docs", derive(serde::Serialize))]
pub struct CatalogEntry {
    pub name: String,
    pub namespace: &'static str,
    pub description: &'static str,
    pub required_params: Vec<String>,
    pub optional_params: Vec<String>,
}

/// Every built-in node, grouped by namespace and sorted by name.
pub fn all_node_docs() -> Vec<CatalogEntry> {
    default_namespaces()
        .iter()
        .flat_map(|(namespace, registry)| {
            registry.specs().into_iter().map(|spec| CatalogEntry {
                name: spec.name.clone(),
                namespace,
                description: spec.description,
                required_params: spec.required_params.iter().cloned().collect(),
                optional_params: spec.optional_params.iter().cloned().collect(),
            })
        })
        .collect()
}

/// Serialises all built-in node documentation to JSON
#[cfg(feature = "docs")]
pub fn export_nodes_json() -> String {
    serde_json::to_string_pretty(&all_node_docs()).expect("NodeDoc serialisation failed")
}

#[cfg(all(test, feature = "docs"))]
mod test {
    /// Regenerate with `cargo run --features docs --bin export-docs > ../docs/content/nodes.json`.
    #[test]
    fn checked_in_catalog_is_current() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/content/nodes.json");
        let Ok(checked_in) = std::fs::read_to_string(path) else {
            return; // Not every checkout (e.g. the published crate) has the docs site.
        };
        assert_eq!(
            checked_in.trim(),
            super::export_nodes_json().trim(),
            "docs/content/nodes.json is stale; regenerate it with \
             `cargo run --features docs --bin export-docs > ../docs/content/nodes.json`"
        );
    }
}
