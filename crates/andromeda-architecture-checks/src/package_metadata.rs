//! Cargo package metadata: what every package takes from the workspace and
//! what each one states for itself.
//!
//! The rules, all readable from the manifests:
//!
//! - The root `[workspace.package]` sets `publish = false`: the workspace ships
//!   a broker binary, and no crate of it goes to a registry.
//! - Every package inherits `version`, `edition`, `authors`, `repository`,
//!   `homepage` and `publish` from the workspace, so a release bumps one
//!   version, the project metadata is written once, and no crate opts into
//!   publishing on its own.
//! - Every package declares a licence, inherited or written out where it
//!   carries code under a further licence.
//! - Every package states what it owns in a non-empty `description`.
//! - Every package takes the workspace lints through `[lints] workspace = true`,
//!   so lint levels are set once for the workspace and never per crate.

use crate::workspace_manifest::check_workspace_manifests;
use toml_edit::{DocumentMut, Item, TableLike};

/// The package fields every package inherits from `[workspace.package]`.
const INHERITED_FIELDS: &[&str] = &["version", "edition", "authors", "repository", "homepage", "publish"];

/// Assert that every workspace manifest satisfies the package metadata rules.
///
/// # Panics
///
/// Panics on any violation, or when a manifest cannot be read or parsed.
pub fn assert_package_metadata(workspace_root: &str) {
    let violations = check_workspace_manifests(workspace_root, check_document);
    assert!(violations.is_empty(), "Cargo package metadata violations:\n{}", violations.join("\n"));
}

/// Check the workspace and package tables one parsed manifest declares.
fn check_document(manifest: &str, document: &DocumentMut, violations: &mut Vec<String>) {
    let table = document.as_table();
    if let Some(workspace) = table.get("workspace").and_then(Item::as_table_like) {
        check_workspace_package(manifest, workspace, violations);
    }
    if let Some(package) = table.get("package").and_then(Item::as_table_like) {
        check_package(manifest, package, violations);
        if !inherits_from_workspace(table.get("lints")) {
            violations.push(format!("{manifest} does not take the workspace lints; write [lints] workspace = true"));
        }
    }
}

/// Check that `[workspace.package]` keeps every inheriting package unpublished.
fn check_workspace_package(manifest: &str, workspace: &dyn TableLike, violations: &mut Vec<String>) {
    let publish = workspace
        .get("package")
        .and_then(Item::as_table_like)
        .and_then(|package| package.get("publish"))
        .and_then(Item::as_bool);
    if publish != Some(false) {
        violations.push(format!("{manifest} [workspace.package] does not set publish = false"));
    }
}

/// Check the inherited fields, the licence and the description of one package.
fn check_package(manifest: &str, package: &dyn TableLike, violations: &mut Vec<String>) {
    for field in INHERITED_FIELDS {
        if !inherits_from_workspace(package.get(field)) {
            violations.push(format!("{manifest} [package] does not inherit {field}; write {field}.workspace = true"));
        }
    }
    let license = package.get("license");
    if !inherits_from_workspace(license) && license.and_then(Item::as_str).is_none_or(str::is_empty) {
        violations.push(format!("{manifest} [package] declares no license"));
    }
    if package.get("description").and_then(Item::as_str).is_none_or(str::is_empty) {
        violations.push(format!("{manifest} [package] has no description"));
    }
}

/// Whether a manifest entry is the `workspace = true` inheritance marker, in
/// its dotted (`version.workspace = true`) or table (`[lints]`) spelling.
fn inherits_from_workspace(item: Option<&Item>) -> bool {
    item.and_then(Item::as_table_like)
        .and_then(|table| table.get("workspace"))
        .and_then(Item::as_bool)
        == Some(true)
}

#[cfg(test)]
mod tests {
    use crate::package_metadata::check_document;
    use pretty_assertions::assert_eq;
    use toml_edit::DocumentMut;

    /// A package manifest whose `[package]` table holds `fields` and which takes
    /// the workspace lints.
    fn package(fields: &str) -> String {
        format!("[package]\nname = \"andromeda-fixture\"\n{fields}\n[lints]\nworkspace = true\n")
    }

    /// The fields a package satisfying every rule declares.
    const COMPLETE: &str = "version.workspace = true\nedition.workspace = true\nlicense.workspace = true\nauthors.workspace = true\nrepository.workspace = true\nhomepage.workspace = true\npublish.workspace = true\ndescription = \"A fixture\"\n";

    fn violations(manifest: &str) -> Vec<String> {
        let document = manifest.parse::<DocumentMut>().expect("fixture parses");
        let mut violations = Vec::new();
        check_document("fixture/Cargo.toml", &document, &mut violations);
        violations
    }

    #[test]
    fn a_package_inheriting_everything_and_describing_itself_is_clean() {
        assert_eq!(violations(&package(COMPLETE)), [] as [String; 0]);
    }

    #[test]
    fn a_package_writing_its_own_license_expression_is_clean() {
        let fields = COMPLETE.replace("license.workspace = true", "license = \"EUPL-1.2 AND MIT\"");
        assert_eq!(violations(&package(&fields)), [] as [String; 0]);
    }

    #[test]
    fn a_package_without_a_license_is_a_violation() {
        let fields = COMPLETE.replace("license.workspace = true\n", "");
        assert_eq!(violations(&package(&fields)), ["fixture/Cargo.toml [package] declares no license"]);
    }

    #[test]
    fn a_package_setting_publish_itself_is_a_violation() {
        let fields = COMPLETE.replace("publish.workspace = true", "publish = false");
        assert_eq!(
            violations(&package(&fields)),
            ["fixture/Cargo.toml [package] does not inherit publish; write publish.workspace = true"]
        );
    }

    #[test]
    fn a_package_pinning_its_own_version_and_edition_is_a_violation_for_each() {
        let fields = COMPLETE
            .replace("version.workspace = true", "version = \"1.0.0\"")
            .replace("edition.workspace = true\n", "");
        assert_eq!(
            violations(&package(&fields)),
            [
                "fixture/Cargo.toml [package] does not inherit version; write version.workspace = true",
                "fixture/Cargo.toml [package] does not inherit edition; write edition.workspace = true",
            ]
        );
    }

    #[test]
    fn a_package_writing_its_own_authors_repository_or_homepage_is_a_violation_for_each() {
        let fields = COMPLETE
            .replace("authors.workspace = true", "authors = [\"Someone\"]")
            .replace("repository.workspace = true\n", "")
            .replace("homepage.workspace = true", "homepage = \"https://example.org\"");
        assert_eq!(
            violations(&package(&fields)),
            [
                "fixture/Cargo.toml [package] does not inherit authors; write authors.workspace = true",
                "fixture/Cargo.toml [package] does not inherit repository; write repository.workspace = true",
                "fixture/Cargo.toml [package] does not inherit homepage; write homepage.workspace = true",
            ]
        );
    }

    #[test]
    fn a_package_with_an_empty_or_missing_description_is_a_violation() {
        let empty = COMPLETE.replace("\"A fixture\"", "\"\"");
        let missing = COMPLETE.replace("description = \"A fixture\"\n", "");
        assert_eq!(violations(&package(&empty)), ["fixture/Cargo.toml [package] has no description"]);
        assert_eq!(violations(&package(&missing)), ["fixture/Cargo.toml [package] has no description"]);
    }

    #[test]
    fn a_package_without_the_workspace_lints_is_a_violation() {
        let manifest = format!("[package]\nname = \"andromeda-fixture\"\n{COMPLETE}");
        assert_eq!(
            violations(&manifest),
            ["fixture/Cargo.toml does not take the workspace lints; write [lints] workspace = true"]
        );
    }

    #[test]
    fn a_package_declaring_its_own_lint_levels_is_a_violation() {
        let manifest = format!("[package]\nname = \"andromeda-fixture\"\n{COMPLETE}\n[lints.rust]\nmissing_docs = \"deny\"\n");
        assert_eq!(
            violations(&manifest),
            ["fixture/Cargo.toml does not take the workspace lints; write [lints] workspace = true"]
        );
    }

    #[test]
    fn a_workspace_package_table_keeping_crates_unpublished_is_clean() {
        let manifest = "[workspace]\nmembers = []\n\n[workspace.package]\nversion = \"1.0.0\"\npublish = false\n";
        assert_eq!(violations(manifest), [] as [String; 0]);
    }

    #[test]
    fn a_workspace_package_table_without_publish_false_is_a_violation() {
        let unset = "[workspace]\nmembers = []\n\n[workspace.package]\nversion = \"1.0.0\"\n";
        let published = "[workspace]\nmembers = []\n\n[workspace.package]\npublish = true\n";
        assert_eq!(violations(unset), ["fixture/Cargo.toml [workspace.package] does not set publish = false"]);
        assert_eq!(violations(published), ["fixture/Cargo.toml [workspace.package] does not set publish = false"]);
    }
}
