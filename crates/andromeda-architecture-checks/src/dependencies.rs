//! Which crates a layer may name, read from a crate's own manifest.

use std::{
    fs,
    path::{Path, PathBuf},
};

/// Focused test-infrastructure crate names that may only be production dependencies of one another.
const TEST_INFRASTRUCTURE_CRATES: &[&str] = &[
    "andromeda-test-client",
    "andromeda-test-fixtures",
    "andromeda-test-harness",
    "andromeda-test-mocks",
    "andromeda-test-tls",
];

/// Which dependency table a declaration came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyKind {
    /// `[dependencies]` — what the crate itself is built against.
    Normal,
    /// `[build-dependencies]` — what its build script is built against.
    Build,
    /// `[dev-dependencies]` — what its tests, benches and examples are built
    /// against.
    Development,
}

impl DependencyKind {
    /// The table header that opens this kind.
    const fn table(self) -> &'static str {
        match self {
            DependencyKind::Normal => "dependencies",
            DependencyKind::Build => "build-dependencies",
            DependencyKind::Development => "dev-dependencies",
        }
    }

    /// The kind a table header names, if it names one.
    fn of_table(table: &str) -> Option<DependencyKind> {
        [DependencyKind::Normal, DependencyKind::Build, DependencyKind::Development]
            .into_iter()
            .find(|kind| kind.table() == table)
    }

    /// Whether a forbidden third-party crate may appear under this kind.
    ///
    /// A driver, a transport or a runtime under `[dependencies]` puts every
    /// layer above the crate above that dependency, which is the coupling the
    /// split exists to remove; the same is true of a build script, whose output
    /// the crate is compiled from. Tests are different: a grammar crate whose
    /// tests are `async` legitimately declares a runtime, and forbidding that
    /// would say nothing about what the crate ships.
    const fn admits_forbidden(self) -> bool {
        matches!(self, DependencyKind::Development)
    }
}

/// The third-party production dependencies the workspace forbids by default.
///
/// Every boundary derives its denylist from this list rather than restating its
/// own, so no crate's can be quietly weaker than another's: the drivers and
/// dialects (`sqlx`, `sea-query`, `sea-query-sqlx`, `geozero`), the transports
/// and async runtime (`axum`, `reqwest`, `rumqttc`, `tokio`), the wire buffer
/// (`bytes`), the configuration and CLI front ends (`figment2`, `clap`), the
/// alternate JSON parser (`sonic-rs`), the type-erased error crate (`anyhow`,
/// whose `Error` the workspace bans in favour of typed enums) and the global
/// allocator (`mimalloc`) are all things a specification or storage-neutral
/// crate has no business shipping.
/// A crate that legitimately ships one subtracts it through
/// [`forbidden_production_dependencies_except`]; nothing adds to the list.
pub const FORBIDDEN_PRODUCTION_DEPENDENCIES: &[&str] = &[
    "anyhow",
    "axum",
    "bytes",
    "clap",
    "figment2",
    "geozero",
    "mimalloc",
    "reqwest",
    "rumqttc",
    "sea-query",
    "sea-query-sqlx",
    "sonic-rs",
    "sqlx",
    "tokio",
];

/// [`FORBIDDEN_PRODUCTION_DEPENDENCIES`] without the dependencies a particular
/// crate is entitled to ship.
///
/// A storage adapter ships `sea-query`; the delivery crate ships `tokio` and
/// `rumqttc`. Such a crate names what it is entitled to here rather than
/// hand-copying the rest of the baseline, so its denylist stays a strict
/// derivation of the shared one and cannot drift below it.
///
/// # Panics
///
/// Panics when `permitted` names a crate the production denylist does not
/// contain: allowing a crate that was never forbidden is a silent no-op.
#[must_use]
pub fn forbidden_production_dependencies_except(permitted: &[&str]) -> Vec<&'static str> {
    for allowed in permitted {
        assert!(
            FORBIDDEN_PRODUCTION_DEPENDENCIES.contains(allowed),
            "{allowed} is not in the production dependency denylist, so carving it out does nothing"
        );
    }
    FORBIDDEN_PRODUCTION_DEPENDENCIES
        .iter()
        .copied()
        .filter(|dependency| !permitted.contains(dependency))
        .collect()
}

/// Assert that a crate declares only what its architecture permits.
///
/// `forbidden` is the third-party production denylist. Normal and build
/// workspace dependencies must appear in `allowed_workspace_dependencies`;
/// workspace dev-dependencies must appear in
/// `allowed_workspace_dev_dependencies`.
///
/// # Panics
///
/// Panics when the crate declares a forbidden dependency, an in-workspace edge
/// its layer does not permit, or when its manifest cannot be read.
pub fn assert_dependency_boundary(crate_root: &str, forbidden: &[&str], allowed_workspace_dependencies: &[&str], allowed_workspace_dev_dependencies: &[&str]) {
    let path = Path::new(crate_root).join("Cargo.toml");
    let manifest = fs::read_to_string(&path);
    // A boundary read from a manifest that is not there is no boundary at all,
    // so the read failure fails the guard; the binding below is what the
    // assertion has already proven present.
    assert!(manifest.is_ok(), "{} could not be read as a manifest: {manifest:?}", path.display());
    if let Ok(manifest) = manifest {
        assert_permitted(
            &declarations(&manifest),
            forbidden,
            allowed_workspace_dependencies,
            allowed_workspace_dev_dependencies,
        );
    }
}

/// Assert that an in-workspace dependency is declared only for tests.
///
/// This remains available to direct callers; the dependency macro expresses
/// the same rule through its separate development allow-list.
///
/// # Panics
///
/// Panics when `dependency` is declared under any non-development table, or when
/// the manifest cannot be read.
pub fn assert_development_only(crate_root: &str, dependency: &str) {
    let path = Path::new(crate_root).join("Cargo.toml");
    let manifest = fs::read_to_string(&path);
    assert!(manifest.is_ok(), "{} could not be read as a manifest: {manifest:?}", path.display());
    if let Ok(manifest) = manifest {
        for (kind, name) in declarations(&manifest) {
            assert!(
                name != dependency || kind == DependencyKind::Development,
                "{dependency} must be a dev-only edge, but it is declared as a {kind:?} dependency"
            );
        }
    }
}

/// Assert that production workspace packages do not ship focused test infrastructure.
///
/// Test-infrastructure crates may compose one another. Every other workspace
/// package may name them only through a development dependency.
///
/// # Panics
///
/// Panics when a production package declares a focused test crate as a normal
/// or build dependency, when a package directory cannot be listed, or when a
/// discovered manifest cannot be read.
pub fn assert_test_infrastructure_is_development_only(workspace_root: &str) {
    let root = Path::new(workspace_root);
    for path in workspace_manifest_paths(root) {
        let manifest = fs::read_to_string(&path);
        assert!(manifest.is_ok(), "{} could not be read as a manifest: {manifest:?}", path.display());
        if let Ok(manifest) = manifest {
            let package = package_name(&manifest);
            if package.is_some_and(|name| TEST_INFRASTRUCTURE_CRATES.contains(&name)) {
                continue;
            }
            for (kind, dependency) in declarations(&manifest) {
                assert!(
                    !TEST_INFRASTRUCTURE_CRATES.contains(&dependency) || kind == DependencyKind::Development,
                    "{} declares test infrastructure {dependency} as a {kind:?} dependency",
                    path.display()
                );
            }
        }
    }
}

/// Discover root and crate manifests without walking generated output.
///
/// A package directory that cannot be listed hides every manifest below it, so
/// the walk fails rather than reporting a boundary it could not see.
///
/// # Panics
///
/// Panics when an existing package directory cannot be listed.
pub(crate) fn workspace_manifest_paths(root: &Path) -> Vec<PathBuf> {
    let mut paths = vec![root.join("Cargo.toml")];
    let mut unreadable = Vec::new();
    for parent in [root.join("crates"), root.join("lib")] {
        if !parent.exists() {
            continue;
        }
        let entries = match fs::read_dir(&parent) {
            Ok(entries) => entries,
            Err(error) => {
                unreadable.push(format!("{} could not be listed: {error}", parent.display()));
                continue;
            }
        };
        for entry in entries {
            let path = match entry {
                Ok(entry) => entry.path().join("Cargo.toml"),
                Err(error) => {
                    unreadable.push(format!("an entry of {} could not be read: {error}", parent.display()));
                    continue;
                }
            };
            if path.exists() {
                paths.push(path);
            }
        }
    }
    assert!(unreadable.is_empty(), "the workspace packages could not be listed:\n{}", unreadable.join("\n"));
    paths.sort();
    paths
}

/// Read the package name from the package table of a Cargo manifest.
fn package_name(manifest: &str) -> Option<&str> {
    let mut in_package = false;
    for line in manifest.lines().map(str::trim) {
        if line == "[package]" {
            in_package = true;
            continue;
        }
        if line.starts_with('[') {
            in_package = false;
            continue;
        }
        if in_package && let Some(value) = line.strip_prefix("name").and_then(|rest| rest.trim_start().strip_prefix('=')) {
            return Some(value.trim().trim_matches('"'));
        }
    }
    None
}

/// The rule itself, over declarations already read.
///
/// Separate from [`assert_dependency_boundary`] so that what the rule rejects can be
/// exercised without a manifest on disk — a guard nothing has ever seen fail is
/// a guard nobody knows works.
fn assert_permitted(
    declared: &[(DependencyKind, &str)],
    forbidden: &[&str],
    allowed_workspace_dependencies: &[&str],
    allowed_workspace_dev_dependencies: &[&str],
) {
    for (kind, name) in declared {
        assert!(kind.admits_forbidden() || !forbidden.contains(name), "this crate must not depend on {name}");
        if name.starts_with("andromeda-") {
            let allowed = match kind {
                DependencyKind::Normal | DependencyKind::Build => allowed_workspace_dependencies.contains(name),
                DependencyKind::Development => *name == "andromeda-architecture-checks" || allowed_workspace_dev_dependencies.contains(name),
            };
            assert!(allowed, "{name} is not an edge this crate's architecture permits ({kind:?} dependency)");
        }
    }
}

/// Every dependency a manifest declares, paired with the table it came from.
///
/// A deliberately small reader rather than a TOML parser, covering the three
/// shapes Cargo manifests take: `name = …` inside a dependency table,
/// `[dependencies.name]`, and a dependency table qualified by a `[target.…]`
/// prefix. Anything it does not recognise yields no declaration, so a shape it
/// cannot read fails open — which is why the crate that declares an edge also
/// carries the test, where the omission is visible.
fn declarations(manifest: &str) -> Vec<(DependencyKind, &str)> {
    let mut declared = Vec::new();
    let mut section: Option<DependencyKind> = None;

    for line in manifest.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let Some(header) = line.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) else {
            if let Some(kind) = section {
                let name = line.split(['=', '.', ' ']).next().unwrap_or_default();
                if !name.is_empty() {
                    declared.push((kind, name));
                }
            }
            continue;
        };

        // A header always closes the previous table, whether or not it opens a
        // dependency one.
        section = None;
        let mut segments = header.split('.');
        let first = segments.next().unwrap_or_default();
        let rest: Vec<&str> = segments.collect();

        match (DependencyKind::of_table(first), rest.as_slice()) {
            // Workspace dependency declarations are version/path aliases, not
            // package edges. The consuming package's own dependency table is
            // the authoritative edge and is checked separately.
            (_, _) if first == "workspace" => {}
            // `[dependencies.serde]` names one dependency and opens a table of
            // its own keys, none of which is a dependency name.
            (Some(kind), [name]) => declared.push((kind, name.trim_matches('"'))),
            // `[dependencies]`
            (Some(kind), []) => section = Some(kind),
            // A dotted table. `[target.'cfg(unix)'.dependencies]` opens a
            // dependency table — its last segment is the kind. The same shape
            // with a trailing name — `[target.'cfg(unix)'.dependencies.sqlx]` —
            // names one dependency whose kind is the penultimate segment; the
            // reader used to fall through this and record nothing at all.
            (_, segments) => match segments {
                [.., last] if DependencyKind::of_table(last).is_some() => section = DependencyKind::of_table(last),
                [.., penultimate, name] => {
                    if let Some(kind) = DependencyKind::of_table(penultimate) {
                        declared.push((kind, name.trim_matches('"')));
                    }
                }
                [_] | [] => {}
            },
        }
    }

    declared
}

#[cfg(test)]
mod tests {
    use crate::dependencies::{DependencyKind, FORBIDDEN_PRODUCTION_DEPENDENCIES, assert_permitted, declarations, forbidden_production_dependencies_except};
    use pretty_assertions::assert_eq;

    /// The layer a specification crate sits at, for the rejection tests below.
    const FORBIDDEN: &[&str] = &["sqlx", "sea-query", "tokio"];
    /// The only in-workspace edge that layer permits, for the same tests.
    const ALLOWED: &[&str] = &["andromeda-ngsi-ld-lexicon"];

    #[test]
    #[should_panic(expected = "must not depend on sqlx")]
    fn a_forbidden_dependency_is_rejected() {
        assert_permitted(&[(DependencyKind::Normal, "sqlx")], FORBIDDEN, ALLOWED, &[]);
    }

    #[test]
    #[should_panic(expected = "must not depend on sqlx")]
    fn a_forbidden_build_dependency_is_rejected() {
        // A build script's dependencies are compiled into what the crate ships.
        assert_permitted(&[(DependencyKind::Build, "sqlx")], FORBIDDEN, ALLOWED, &[]);
    }

    #[test]
    fn a_forbidden_crate_is_admitted_as_a_dev_dependency() {
        // A grammar crate whose tests are async declares a runtime and ships none.
        assert_permitted(&[(DependencyKind::Development, "tokio")], FORBIDDEN, &[], &[]);
    }

    #[test]
    #[should_panic(expected = "andromeda-ngsi-ld-model is not an edge")]
    fn an_in_workspace_edge_outside_the_allow_list_is_rejected() {
        assert_permitted(&[(DependencyKind::Normal, "andromeda-ngsi-ld-model")], FORBIDDEN, ALLOWED, &[]);
    }

    #[test]
    #[should_panic(expected = "andromeda-ngsi-ld-model is not an edge")]
    fn an_in_workspace_edge_declared_only_for_tests_is_still_an_edge() {
        // The half the previous reader could not see: the model/query inversion
        // held one end of itself in a `[dev-dependencies]` table.
        assert_permitted(&[(DependencyKind::Development, "andromeda-ngsi-ld-model")], FORBIDDEN, &[], ALLOWED);
    }

    #[test]
    fn a_permitted_edge_passes_in_every_table() {
        for kind in [DependencyKind::Normal, DependencyKind::Build, DependencyKind::Development] {
            assert_permitted(&[(kind, "andromeda-ngsi-ld-lexicon")], FORBIDDEN, ALLOWED, ALLOWED);
        }
    }

    #[test]
    fn every_dependency_table_is_read_and_attributed_to_its_kind() {
        let manifest = "\
[package]
name = \"example\"

[dependencies]
serde = { version = \"1.0.0\" }
andromeda-ngsi-ld-lexicon.workspace = true

[build-dependencies]
static-regular-grammar = \"3.0.0\"

[dev-dependencies]
tokio = { version = \"1.0.0\", features = [\"full\"] }

[lints]
workspace = true
";
        assert_eq!(
            declarations(manifest),
            vec![
                (DependencyKind::Normal, "serde"),
                (DependencyKind::Normal, "andromeda-ngsi-ld-lexicon"),
                (DependencyKind::Build, "static-regular-grammar"),
                (DependencyKind::Development, "tokio"),
            ]
        );
    }

    #[test]
    fn a_dependency_declared_as_its_own_table_is_read() {
        // The shape that used to slip past entirely: the reader split on the
        // literal `[dependencies]`, which `[dependencies.sqlx]` is not.
        let manifest = "[dependencies.sqlx]\nversion = \"0.8.6\"\n";
        assert_eq!(declarations(manifest), vec![(DependencyKind::Normal, "sqlx")]);
    }

    #[test]
    fn a_target_qualified_dependency_table_is_read() {
        let manifest = "[target.'cfg(unix)'.dev-dependencies]\nnix = \"0.29.0\"\n";
        assert_eq!(declarations(manifest), vec![(DependencyKind::Development, "nix")]);
    }

    #[test]
    fn a_target_qualified_single_dependency_table_is_read() {
        // The shape the reader used to drop on the floor: a single dependency
        // named as its own target-qualified table, where the kind is the
        // penultimate segment rather than the last.
        let manifest = "[target.'cfg(unix)'.dependencies.sqlx]\nversion = \"0.8.6\"\n";
        assert_eq!(declarations(manifest), vec![(DependencyKind::Normal, "sqlx")]);
    }

    #[test]
    fn a_table_that_is_not_a_dependency_table_declares_nothing() {
        let manifest = "[package.metadata.docs.rs]\nall-features = true\n";
        assert_eq!(declarations(manifest), [] as [(DependencyKind, &str); 0]);
    }

    #[test]
    fn a_test_only_runtime_is_not_a_layering_violation_but_a_test_only_edge_is() {
        // Both halves of the rule the previous reader could not express: a
        // grammar crate whose tests are async legitimately declares `tokio`,
        // and a dev-dependency on another workspace crate is still an edge.
        let declared = declarations("[dev-dependencies]\ntokio = \"1.0.0\"\nandromeda-ngsi-ld-model.workspace = true\n");
        assert_eq!(
            declared,
            vec![(DependencyKind::Development, "tokio"), (DependencyKind::Development, "andromeda-ngsi-ld-model"),]
        );
        assert!(declared.iter().all(|(kind, _)| kind.admits_forbidden()));
    }

    #[test]
    fn the_baseline_forbids_the_runtime_transport_dialect_and_allocator_gaps() {
        // The four the per-crate template omitted, which is what let an async
        // runtime into a passive crate and a dialect binder past every guard.
        for gap in ["anyhow", "rumqttc", "sea-query-sqlx", "sonic-rs", "mimalloc"] {
            assert!(FORBIDDEN_PRODUCTION_DEPENDENCIES.contains(&gap), "{gap} must be forbidden by default");
        }
    }

    #[test]
    fn a_carve_out_removes_only_the_named_crate_and_keeps_the_rest_of_the_baseline() {
        let forbidden = forbidden_production_dependencies_except(&["sea-query"]);
        assert!(!forbidden.contains(&"sea-query"), "the carved-out crate is still forbidden");
        assert!(forbidden.contains(&"sqlx"), "an uncarved baseline crate was dropped");
        assert_eq!(forbidden.len(), FORBIDDEN_PRODUCTION_DEPENDENCIES.len() - 1);
    }

    #[test]
    #[should_panic(expected = "is not in the production dependency denylist")]
    fn carving_out_a_crate_the_baseline_never_forbade_is_rejected() {
        // A carve-out that subtracts nothing is a mistake, not a permission.
        let _ = forbidden_production_dependencies_except(&["serde"]);
    }
}
