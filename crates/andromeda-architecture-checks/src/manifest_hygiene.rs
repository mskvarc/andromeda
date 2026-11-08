//! Cargo manifest hygiene: version precision, ordering, and where each kind of
//! dependency is allowed to live.
//!
//! Four rules the workspace holds, all readable from the manifests themselves:
//!
//! - Every external dependency pins a full `major.minor.patch` version: never a
//!   partial `"1"` or `"1.2"`, and never a range operator.
//! - Every dependency table is in ascending order by dependency name.
//! - The root `[workspace.dependencies]` holds only in-tree path dependencies
//!   (`andromeda-*` with a `path`); an external crate never belongs there.
//! - `dep.workspace = true` is only ever an in-tree `andromeda-*` edge; an
//!   external crate is declared inline in each crate that uses it, so it never
//!   inherits from the workspace, and an in-tree crate is never spelled inline.
//!
//! The rules apply to `[dependencies]`, `[dev-dependencies]`,
//! `[build-dependencies]`, and their `[target.'…'.…]` forms alike.

use crate::workspace_manifest::check_workspace_manifests;
use toml_edit::{DocumentMut, Item, TableLike};

/// Assert that every workspace manifest satisfies the hygiene rules.
///
/// # Panics
///
/// Panics on any hygiene violation, or when a manifest cannot be read or parsed.
pub fn assert_manifest_hygiene(workspace_root: &str) {
    let violations = check_workspace_manifests(workspace_root, check_document);
    assert!(violations.is_empty(), "Cargo manifest hygiene violations:\n{}", violations.join("\n"));
}

/// The dependency-table kinds a crate manifest declares.
const DEPENDENCY_TABLES: &[&str] = &["dependencies", "dev-dependencies", "build-dependencies"];

/// Check every dependency table one parsed manifest declares.
fn check_document(manifest: &str, document: &DocumentMut, violations: &mut Vec<String>) {
    let table = document.as_table();
    for kind in DEPENDENCY_TABLES {
        if let Some(dependencies) = table.get(kind).and_then(Item::as_table_like) {
            check_dependency_table(manifest, kind, dependencies, TableRole::Crate, violations);
        }
    }
    if let Some(targets) = table.get("target").and_then(Item::as_table_like) {
        for (target, item) in targets.iter() {
            if let Some(target_table) = item.as_table_like() {
                for kind in DEPENDENCY_TABLES {
                    if let Some(dependencies) = target_table.get(kind).and_then(Item::as_table_like) {
                        check_dependency_table(manifest, &format!("target.{target}.{kind}"), dependencies, TableRole::Crate, violations);
                    }
                }
            }
        }
    }
    if let Some(workspace) = table.get("workspace").and_then(Item::as_table)
        && let Some(dependencies) = workspace.get("dependencies").and_then(Item::as_table_like)
    {
        check_dependency_table(manifest, "workspace.dependencies", dependencies, TableRole::Workspace, violations);
    }
}

/// Whether a dependency table is a crate's own edges or the root workspace
/// alias table, which admits only in-tree path dependencies.
#[derive(Clone, Copy)]
enum TableRole {
    /// A `[dependencies]`/`[dev-dependencies]`/`[build-dependencies]` table.
    Crate,
    /// The root `[workspace.dependencies]` table.
    Workspace,
}

/// Check ordering and per-entry rules for one dependency table.
fn check_dependency_table(manifest: &str, table: &str, dependencies: &dyn TableLike, role: TableRole, violations: &mut Vec<String>) {
    let mut previous: Option<&str> = None;
    for (name, item) in dependencies.iter() {
        if let Some(previous) = previous
            && name < previous
        {
            violations.push(format!("{manifest} [{table}] is out of order: {name} precedes {previous}"));
        }
        previous = Some(name);
        check_dependency(manifest, table, name, item, role, violations);
    }
}

/// Check the placement and version rules for one dependency entry.
fn check_dependency(manifest: &str, table: &str, name: &str, item: &Item, role: TableRole, violations: &mut Vec<String>) {
    let in_tree = name.starts_with("andromeda-");
    let from_workspace = bool_field(item, "workspace") == Some(true);
    let has_path = field_present(item, "path");

    match role {
        TableRole::Workspace => {
            if !in_tree || !has_path {
                violations.push(format!(
                    "{manifest} [{table}] holds {name}; the workspace table takes only in-tree path dependencies"
                ));
            }
        }
        TableRole::Crate => {
            if in_tree {
                if !from_workspace {
                    violations.push(format!(
                        "{manifest} [{table}] declares in-tree {name} inline; reference it as {name}.workspace = true"
                    ));
                }
            } else if from_workspace {
                violations.push(format!(
                    "{manifest} [{table}] inherits external {name} from the workspace; declare it inline with its own version"
                ));
            } else if has_path {
                violations.push(format!(
                    "{manifest} [{table}] declares {name} by path; in-tree crates go through [workspace.dependencies]"
                ));
            } else {
                match version_of(item) {
                    Some(version) if is_full_semver(version) => {}
                    Some(version) => violations.push(format!(
                        "{manifest} [{table}] pins {name} = \"{version}\"; use a full major.minor.patch version"
                    )),
                    None => violations.push(format!("{manifest} [{table}] declares {name} without a full major.minor.patch version")),
                }
            }
        }
    }
}

/// The version string a dependency entry carries, whether written as a bare
/// string or as the `version` field of an inline table or its own table.
fn version_of(item: &Item) -> Option<&str> {
    if let Some(version) = item.as_str() {
        return Some(version);
    }
    item.as_table_like().and_then(|table| table.get("version")).and_then(Item::as_str)
}

/// Whether a table-like dependency entry sets a boolean field.
fn bool_field(item: &Item, field: &str) -> Option<bool> {
    item.as_table_like().and_then(|table| table.get(field)).and_then(Item::as_bool)
}

/// Whether a table-like dependency entry declares a field at all.
fn field_present(item: &Item, field: &str) -> bool {
    item.as_table_like().is_some_and(|table| table.get(field).is_some())
}

/// Whether a version string is an exact `major.minor.patch`, optionally carrying
/// a pre-release or build suffix, with no range operator.
///
/// This rejects the partial `"1"` and `"1.2"` forms and every `^`, `~`, `>=`,
/// `*` range spelling, since each of those either under-pins the version or is
/// not a version at all.
fn is_full_semver(version: &str) -> bool {
    let core = version.split(['-', '+']).next().unwrap_or_default();
    let components: Vec<&str> = core.split('.').collect();
    components.len() == 3
        && components
            .iter()
            .all(|component| !component.is_empty() && component.bytes().all(|byte| byte.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use crate::manifest_hygiene::{check_document, is_full_semver};
    use pretty_assertions::assert_eq;
    use toml_edit::DocumentMut;

    fn violations(manifest: &str) -> Vec<String> {
        let document = manifest.parse::<DocumentMut>().unwrap_or_else(|error| panic!("fixture parses: {error}"));
        let mut violations = Vec::new();
        check_document("fixture/Cargo.toml", &document, &mut violations);
        violations
    }

    #[test]
    fn a_full_semver_is_accepted_and_partials_and_ranges_are_not() {
        assert!(is_full_semver("1.2.3"));
        assert!(is_full_semver("0.25.15"));
        assert!(is_full_semver("1.2.3-alpha.1"));
        assert!(!is_full_semver("1"));
        assert!(!is_full_semver("1.2"));
        assert!(!is_full_semver("^1.2.3"));
        assert!(!is_full_semver("*"));
    }

    #[test]
    fn a_clean_table_has_no_violations() {
        let manifest = "[dependencies]\nandromeda-entity.workspace = true\naxum = \"0.8.9\"\nserde = { version = \"1.0.229\", features = [\"derive\"] }\n";
        assert_eq!(violations(manifest), [] as [String; 0]);
    }

    #[test]
    fn an_out_of_order_table_is_a_violation() {
        let manifest = "[dependencies]\ntracing = \"0.1.44\"\nthiserror = \"2.0.20\"\n";
        assert!(violations(manifest).iter().any(|violation| violation.contains("out of order")));
    }

    #[test]
    fn a_partial_version_is_a_violation() {
        let manifest = "[dependencies]\nserde = \"1\"\n";
        assert!(violations(manifest).iter().any(|violation| violation.contains("full major.minor.patch")));
    }

    #[test]
    fn an_external_crate_inheriting_from_the_workspace_is_a_violation() {
        let manifest = "[dependencies]\nserde.workspace = true\n";
        assert!(violations(manifest).iter().any(|violation| violation.contains("inherits external serde")));
    }

    #[test]
    fn an_in_tree_crate_declared_inline_is_a_violation() {
        let manifest = "[dependencies]\nandromeda-entity = { path = \"../andromeda-entity\" }\n";
        assert!(
            violations(manifest)
                .iter()
                .any(|violation| violation.contains("declares in-tree andromeda-entity inline"))
        );
    }

    #[test]
    fn an_external_crate_in_the_workspace_table_is_a_violation() {
        let manifest = "[workspace.dependencies]\nserde = \"1.0.229\"\n";
        assert!(
            violations(manifest)
                .iter()
                .any(|violation| violation.contains("takes only in-tree path dependencies"))
        );
    }

    #[test]
    fn an_in_tree_path_dependency_in_the_workspace_table_is_clean() {
        let manifest = "[workspace.dependencies]\nandromeda-entity = { path = \"crates/andromeda-entity\" }\n";
        assert_eq!(violations(manifest), [] as [String; 0]);
    }
}
