//! The configuration schema crate is the bottom of the in-tree graph.
//!
//! Every domain crate takes its section from `andromeda-configuration`, so a
//! value type a section is built from belongs in that crate. An in-tree edge out
//! of it would let a crate both feed the schema and read its section from it,
//! and a dev edge would close that loop for tests, which Cargo does not refuse.
//! The only edge it keeps is the architecture checks its own tests run.
//!
//! The crate also names no HTTP crate. A section names how the HTTP layer
//! behaves (a mode, a layout) and the HTTP layer carries that behaviour out,
//! so header maps, header values and middleware never enter the schema.

use crate::dependencies::{DependencyKind, declarations};
use std::{fs, path::Path};

/// The crate that owns the configuration schema.
const CONFIGURATION_CRATE: &str = "andromeda-configuration";

/// The one in-tree crate the configuration crate may name, for its tests.
const ARCHITECTURE_CHECKS: &str = "andromeda-architecture-checks";

/// The HTTP crates the configuration crate never names, in any table.
const HTTP_CRATES: &[&str] = &[
    "axum",
    "headers",
    "http",
    "http-body",
    "http-body-util",
    "hyper",
    "hyper-util",
    "tower",
    "tower-http",
];

/// Assert that the configuration crate declares no in-tree dependency and no
/// HTTP crate.
///
/// # Panics
///
/// Panics when the configuration manifest cannot be read, when it names an
/// in-tree crate other than the architecture checks under `[dev-dependencies]`,
/// or when it names an HTTP crate.
pub fn assert_configuration_is_a_leaf(workspace_root: &str) {
    let path = Path::new(workspace_root).join("crates").join(CONFIGURATION_CRATE).join("Cargo.toml");
    let manifest = fs::read_to_string(&path);
    assert!(manifest.is_ok(), "{} could not be read as a manifest: {manifest:?}", path.display());
    if let Ok(manifest) = manifest {
        let edges = refused_edges(&declarations(&manifest));
        assert!(
            edges.is_empty(),
            "{CONFIGURATION_CRATE} must not depend on another in-tree crate or on an HTTP crate; move the type it needs into it, or keep the behaviour in the crate that owns it:\n{}",
            edges.join("\n")
        );
    }
}

/// Every edge the rule refuses, rendered with its table.
fn refused_edges(declared: &[(DependencyKind, &str)]) -> Vec<String> {
    declared
        .iter()
        .filter(|(kind, name)| {
            let in_tree = name.starts_with("andromeda-") && !(*kind == DependencyKind::Development && *name == ARCHITECTURE_CHECKS);
            in_tree || HTTP_CRATES.contains(name)
        })
        .map(|(kind, name)| format!("{name} ({kind:?} dependency)"))
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::{configuration_layering::refused_edges, dependencies::declarations};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_manifest_with_only_external_crates_and_the_checks_for_tests_is_a_leaf() {
        let manifest = "[dependencies]\nserde = \"1.0.229\"\n[dev-dependencies]\nandromeda-architecture-checks.workspace = true\n";
        assert_eq!(refused_edges(&declarations(manifest)), [] as [String; 0]);
    }

    #[test]
    fn an_in_tree_production_edge_is_refused() {
        let manifest = "[dependencies]\nandromeda-retry.workspace = true\n";
        assert_eq!(refused_edges(&declarations(manifest)), ["andromeda-retry (Normal dependency)"]);
    }

    #[test]
    fn an_in_tree_dev_edge_other_than_the_checks_is_refused() {
        let manifest = "[dev-dependencies]\nandromeda-ngsi-ld-lexicon.workspace = true\n";
        assert_eq!(refused_edges(&declarations(manifest)), ["andromeda-ngsi-ld-lexicon (Development dependency)"]);
    }

    #[test]
    fn the_checks_as_a_production_edge_is_refused() {
        let manifest = "[dependencies]\nandromeda-architecture-checks.workspace = true\n";
        assert_eq!(refused_edges(&declarations(manifest)), ["andromeda-architecture-checks (Normal dependency)"]);
    }

    #[test]
    fn an_in_tree_build_edge_is_refused() {
        let manifest = "[build-dependencies]\nandromeda-retry.workspace = true\n";
        assert_eq!(refused_edges(&declarations(manifest)), ["andromeda-retry (Build dependency)"]);
    }

    #[test]
    fn an_http_crate_is_refused_in_every_table() {
        let manifest = "[dependencies]\nhttp = \"1.5.0\"\n[dev-dependencies]\ntower = \"0.5.3\"\n[build-dependencies]\nhyper = \"1.8.1\"\n";
        assert_eq!(
            refused_edges(&declarations(manifest)),
            ["http (Normal dependency)", "tower (Development dependency)", "hyper (Build dependency)"]
        );
    }
}
