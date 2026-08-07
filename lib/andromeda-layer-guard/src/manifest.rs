//! Which crates a layer may name, read from a crate's own manifest.

use std::{fs, path::Path};

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

/// The third-party crates the workspace forbids by default, in one place.
///
/// Every layer derives its denylist from this list rather than restating its
/// own, so no crate's can be quietly weaker than another's: the drivers and
/// dialects (`sqlx`, `sea-query`, `sea-query-binder`, `geozero`), the transports
/// and async runtime (`axum`, `reqwest`, `rumqttc`, `tokio`), the wire buffer
/// (`bytes`), the configuration and CLI front ends (`confique`, `clap`), the
/// alternate JSON parser (`sonic-rs`) and the global allocator (`mimalloc`) are
/// all things a specification or storage-neutral crate has no business shipping.
/// A crate that legitimately ships one subtracts it through
/// [`baseline_forbidden_except`]; nothing adds to the list.
pub const BASELINE_FORBIDDEN: &[&str] = &[
    "axum",
    "bytes",
    "clap",
    "confique",
    "geozero",
    "mimalloc",
    "reqwest",
    "rumqttc",
    "sea-query",
    "sea-query-binder",
    "sonic-rs",
    "sqlx",
    "tokio",
];

/// [`BASELINE_FORBIDDEN`] without the crates a particular layer is entitled to
/// ship.
///
/// A storage adapter ships `sea-query`; the delivery crate ships `tokio` and
/// `rumqttc`. Such a crate names what it is entitled to here rather than
/// hand-copying the rest of the baseline, so its denylist stays a strict
/// derivation of the shared one and cannot drift below it.
///
/// # Panics
///
/// Panics when `permitted` names a crate the baseline does not forbid: carving
/// out a crate that was never on the list is a silent no-op, so it is almost
/// certainly a typo or a stale exception.
pub fn baseline_forbidden_except(permitted: &[&str]) -> Vec<&'static str> {
    for allowed in permitted {
        assert!(
            BASELINE_FORBIDDEN.contains(allowed),
            "{allowed} is not in the baseline denylist, so carving it out does nothing"
        );
    }
    BASELINE_FORBIDDEN
        .iter()
        .copied()
        .filter(|dependency| !permitted.contains(dependency))
        .collect()
}

/// Assert that a crate declares only what its layer permits.
///
/// `forbidden` is the third-party denylist, applied to everything the crate
/// ships. `allowed_in_workspace` is the positive list of in-workspace edges,
/// applied to **every** table including `[dev-dependencies]` — a layering edge
/// is a layering edge whichever table declares it, and reading only
/// `[dependencies]` is precisely how `andromeda-ngsi-ld-query` depending on the
/// data model stayed invisible while a dev-dependency held the other end of it.
///
/// # Panics
///
/// Panics when the crate declares a forbidden dependency, an in-workspace edge
/// its layer does not permit, or when its manifest cannot be read.
pub fn assert_layer(crate_root: &str, forbidden: &[&str], allowed_in_workspace: &[&str]) {
    let path = Path::new(crate_root).join("Cargo.toml");
    let manifest = fs::read_to_string(&path).unwrap_or_else(|error| unreachable!("{} is readable: {error}", path.display()));
    assert_permitted(&declarations(&manifest), forbidden, allowed_in_workspace);
}

/// Assert that an in-workspace dependency is declared only for tests.
///
/// [`assert_layer`]'s allow-list permits an edge in *any* table, so it cannot
/// express an edge that a layer admits for its tests but must never ship — the
/// section 8 model→representation edge is one such: `andromeda-jsonld` builds
/// its tests against `andromeda-ngsi-ld-representation` but must not depend on
/// it. A crate that restricts an edge to `[dev-dependencies]` pins that here, so
/// promoting the edge to a normal dependency fails the crate's own boundary test.
///
/// # Panics
///
/// Panics when `dependency` is declared under any non-development table, or when
/// the manifest cannot be read.
pub fn assert_dev_only(crate_root: &str, dependency: &str) {
    let path = Path::new(crate_root).join("Cargo.toml");
    let manifest = fs::read_to_string(&path).unwrap_or_else(|error| unreachable!("{} is readable: {error}", path.display()));
    for (kind, name) in declarations(&manifest) {
        assert!(
            name != dependency || kind == DependencyKind::Development,
            "{dependency} must be a dev-only edge, but it is declared as a {kind:?} dependency"
        );
    }
}

/// The rule itself, over declarations already read.
///
/// Separate from [`assert_layer`] so that what the rule rejects can be
/// exercised without a manifest on disk — a guard nothing has ever seen fail is
/// a guard nobody knows works.
fn assert_permitted(declared: &[(DependencyKind, &str)], forbidden: &[&str], allowed_in_workspace: &[&str]) {
    for (kind, name) in declared {
        assert!(kind.admits_forbidden() || !forbidden.contains(name), "this crate must not depend on {name}");
        if name.starts_with("andromeda-") {
            assert!(
                allowed_in_workspace.contains(name),
                "{name} is not an edge this crate's layer permits ({kind:?} dependency)"
            );
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
    use crate::manifest::{BASELINE_FORBIDDEN, DependencyKind, assert_permitted, baseline_forbidden_except, declarations};

    /// The layer a specification crate sits at, for the rejection tests below.
    const FORBIDDEN: &[&str] = &["sqlx", "sea-query", "tokio"];
    /// The only in-workspace edge that layer permits, for the same tests.
    const ALLOWED: &[&str] = &["andromeda-ngsi-ld-lexicon"];

    #[test]
    #[should_panic(expected = "must not depend on sqlx")]
    fn a_forbidden_dependency_is_rejected() {
        assert_permitted(&[(DependencyKind::Normal, "sqlx")], FORBIDDEN, ALLOWED);
    }

    #[test]
    #[should_panic(expected = "must not depend on sqlx")]
    fn a_forbidden_build_dependency_is_rejected() {
        // A build script's dependencies are compiled into what the crate ships.
        assert_permitted(&[(DependencyKind::Build, "sqlx")], FORBIDDEN, ALLOWED);
    }

    #[test]
    fn a_forbidden_crate_is_admitted_as_a_dev_dependency() {
        // A grammar crate whose tests are async declares a runtime and ships none.
        assert_permitted(&[(DependencyKind::Development, "tokio")], FORBIDDEN, ALLOWED);
    }

    #[test]
    #[should_panic(expected = "andromeda-ngsi-ld-model is not an edge")]
    fn an_in_workspace_edge_outside_the_allow_list_is_rejected() {
        assert_permitted(&[(DependencyKind::Normal, "andromeda-ngsi-ld-model")], FORBIDDEN, ALLOWED);
    }

    #[test]
    #[should_panic(expected = "andromeda-ngsi-ld-model is not an edge")]
    fn an_in_workspace_edge_declared_only_for_tests_is_still_an_edge() {
        // The half the previous reader could not see: the model/query inversion
        // held one end of itself in a `[dev-dependencies]` table.
        assert_permitted(&[(DependencyKind::Development, "andromeda-ngsi-ld-model")], FORBIDDEN, ALLOWED);
    }

    #[test]
    fn a_permitted_edge_passes_in_every_table() {
        for kind in [DependencyKind::Normal, DependencyKind::Build, DependencyKind::Development] {
            assert_permitted(&[(kind, "andromeda-ngsi-ld-lexicon")], FORBIDDEN, ALLOWED);
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
        assert!(declarations(manifest).is_empty());
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
        for gap in ["rumqttc", "sea-query-binder", "sonic-rs", "mimalloc"] {
            assert!(BASELINE_FORBIDDEN.contains(&gap), "{gap} must be forbidden by default");
        }
    }

    #[test]
    fn a_carve_out_removes_only_the_named_crate_and_keeps_the_rest_of_the_baseline() {
        let forbidden = baseline_forbidden_except(&["sea-query"]);
        assert!(!forbidden.contains(&"sea-query"), "the carved-out crate is still forbidden");
        assert!(forbidden.contains(&"sqlx"), "an uncarved baseline crate was dropped");
        assert_eq!(forbidden.len(), BASELINE_FORBIDDEN.len() - 1);
    }

    #[test]
    #[should_panic(expected = "is not in the baseline denylist")]
    fn carving_out_a_crate_the_baseline_never_forbade_is_rejected() {
        // A carve-out that subtracts nothing is a mistake, not a permission.
        let _ = baseline_forbidden_except(&["serde"]);
    }
}
