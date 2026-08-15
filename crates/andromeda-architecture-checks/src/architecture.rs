//! Permanent source guards for broker-wide structural invariants.

use crate::source_scan::{is_test_only, workspace_sources};
use quote::ToTokens;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};
use syn::{
    Item,
    ItemImpl,
    ItemUse,
    Visibility,
    visit::{Visit, visit_ident, visit_item, visit_item_impl, visit_lit_str},
};

/// Assert the structural invariants retired by the seventh architecture pass.
///
/// The scan covers production Rust below `src/`, `crates/`, and `lib/`. It
/// rejects public re-exports, then verifies that the retired composition and
/// dumping-ground files stay absent.
///
/// # Panics
///
/// Panics when a forbidden source shape is found or a retired path reappears.
pub fn assert_retired_architecture_invariants(workspace_root: &str) {
    let root = Path::new(workspace_root);
    let mut violations = Vec::new();
    for source in workspace_sources(root) {
        let mut visitor = ArchitectureVisitor {
            path: &source.workspace_path,
            violations: Vec::new(),
        };
        visitor.visit_file(&source.syntax);
        violations.extend(visitor.violations);
    }
    violations.sort();
    violations.dedup();

    let retired_paths = [
        "src/core",
        "src/api",
        "src/broker",
        "src/config",
        "src/error",
        "src/error.rs",
        "src/event",
        "src/http",
        "src/http/handler/types.rs",
        "src/api/router.rs",
        "src/api/tenant.rs",
        "src/server.rs",
        "src/version",
        "src/worker",
        "crates/andromeda-workers",
        "src/service/temporal.rs",
        "src/repository/temporal/repo.rs",
    ];
    retired_paths.iter().for_each(|path| {
        if root.join(path).exists() {
            violations.push(format!("retired path exists: {path}"));
        }
    });

    let config_path = root.join("config/andromeda.toml");
    let config = fs::read_to_string(&config_path).unwrap_or_else(|error| unreachable!("{} is readable: {error}", config_path.display()));
    if config.contains("source_identity") {
        violations.push("config/andromeda.toml contains retired source_identity key".to_owned());
    }

    let violations = violations.into_iter().collect::<BTreeSet<_>>();
    assert!(
        violations.is_empty(),
        "retired architecture invariant violations:\n{}",
        violations.into_iter().collect::<Vec<_>>().join("\n")
    );
}

/// Assert source boundaries specific to the focused integration-test infrastructure.
///
/// # Panics
///
/// Panics when root tests contain direct global truncation, when the retired
/// umbrella crate name returns, or when a focused test crate introduces a
/// generic dumping-ground module.
pub fn assert_test_infrastructure_invariants(workspace_root: &str) {
    let root = Path::new(workspace_root);
    let mut violations = Vec::new();
    scan_test_sources(root, &mut violations);
    scan_test_crate_modules(root, &mut violations);
    let retired_manifest = root.join("lib/andromeda-test-common/Cargo.toml");
    if retired_manifest.exists() {
        violations.push("retired lib/andromeda-test-common manifest exists".to_owned());
    }
    let violations = violations.into_iter().collect::<BTreeSet<_>>();
    assert!(
        violations.is_empty(),
        "test infrastructure invariant violations:\n{}",
        violations.into_iter().collect::<Vec<_>>().join("\n")
    );
}

/// Scan root integration-test sources for retired names and direct global SQL resets.
fn scan_test_sources(root: &Path, violations: &mut Vec<String>) {
    let tests = root.join("tests");
    for path in rust_files_below(&tests) {
        let source = fs::read_to_string(&path).unwrap_or_else(|error| unreachable!("{} is readable: {error}", path.display()));
        let relative = path.strip_prefix(root).unwrap_or(&path).display();
        if source.contains(concat!("andromeda_test_", "common")) || source.contains(concat!("andromeda-test-", "common")) {
            violations.push(format!("{relative} contains the retired test-common name"));
        }
        if source.contains("TcpListener::bind") || source.contains("axum::serve") {
            violations.push(format!("{relative} binds a callback listener outside andromeda-test-mocks"));
        }
        let syntax = syn::parse_file(&source).unwrap_or_else(|error| unreachable!("{} parses as Rust: {error}", path.display()));
        let mut visitor = TestInfrastructureVisitor {
            path: relative.to_string(),
            violations: Vec::new(),
        };
        visitor.visit_file(&syntax);
        violations.extend(visitor.violations);
    }
}

/// Reject generic module names inside the four focused test crates.
fn scan_test_crate_modules(root: &Path, violations: &mut Vec<String>) {
    let crates = root.join("crates");
    for crate_name in [
        "andromeda-test-client",
        "andromeda-test-fixtures",
        "andromeda-test-harness",
        "andromeda-test-mocks",
    ] {
        let source_root = crates.join(crate_name).join("src");
        for path in rust_files_below(&source_root) {
            let stem = path.file_stem().and_then(|value| value.to_str()).unwrap_or_default();
            let parent = path.parent().and_then(Path::file_name).and_then(|value| value.to_str()).unwrap_or_default();
            if ["common", "utils", "helpers", "shared", "misc"].contains(&stem)
                || (stem == "mod" && ["common", "utils", "helpers", "shared", "misc"].contains(&parent))
            {
                violations.push(format!("{} uses a generic shared-infrastructure module name", path.display()));
            }
        }
    }
}

/// Recursively discover Rust files below one existing directory.
fn rust_files_below(root: &Path) -> Vec<PathBuf> {
    if !root.exists() {
        return Vec::new();
    }
    let mut files = Vec::new();
    let mut directories = vec![root.to_path_buf()];
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(&directory).unwrap_or_else(|error| unreachable!("{} is readable: {error}", directory.display())) {
            let path = entry
                .unwrap_or_else(|error| unreachable!("an entry of {} is readable: {error}", directory.display()))
                .path();
            if path.is_dir() {
                directories.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

/// SQL-string visitor for root integration-test source.
struct TestInfrastructureVisitor {
    /// Workspace-relative source path.
    path: String,
    /// Violations found in the source.
    violations: Vec<String>,
}

impl<'ast> Visit<'ast> for TestInfrastructureVisitor {
    fn visit_lit_str(&mut self, literal: &'ast syn::LitStr) {
        if literal.value().trim_start().to_ascii_uppercase().starts_with("TRUNCATE ") {
            self.violations.push(format!("{} contains direct global TRUNCATE SQL", self.path));
        }
        visit_lit_str(self, literal);
    }
}

/// Find public re-exports and over-qualified inline paths in one syntax tree.
struct ArchitectureVisitor<'path> {
    /// Workspace-relative file currently scanned.
    path: &'path str,
    /// Exact violations found in the file.
    violations: Vec<String>,
}

impl<'ast> Visit<'ast> for ArchitectureVisitor<'_> {
    fn visit_item(&mut self, item: &'ast Item) {
        if item_is_test_only(item) {
            return;
        }
        visit_item(self, item);
    }

    fn visit_item_use(&mut self, item: &'ast ItemUse) {
        if matches!(item.vis, Visibility::Public(_)) {
            self.violations
                .push(format!("{} contains public re-export {}", self.path, item.to_token_stream()));
        }
    }

    fn visit_item_impl(&mut self, item: &'ast ItemImpl) {
        if let Some((trait_path, _)) = &item.trait_
            && let Some(trait_name) = trait_path.segments.last().map(|segment| segment.ident.to_string())
            && let Some(owner) = technical_http_trait_owner(&trait_name)
            && !self.path.starts_with(owner)
        {
            self.violations
                .push(format!("{} implements {trait_name} outside its technical owner {owner}", self.path));
        }
        visit_item_impl(self, item);
    }

    fn visit_ident(&mut self, identifier: &'ast syn::Ident) {
        if identifier == "AppState" {
            self.violations.push(format!("{} contains retired AppState identifier", self.path));
        }
        if identifier == "AndromedaError" {
            self.violations.push(format!("{} contains retired AndromedaError identifier", self.path));
        }
        visit_ident(self, identifier);
    }

    fn visit_lit_str(&mut self, literal: &'ast syn::LitStr) {
        if literal.value() == concat!("ANDROMEDA_INFO_", "SOURCE_IDENTITY") {
            self.violations.push(format!("{} contains retired source-identity environment key", self.path));
        }
        visit_lit_str(self, literal);
    }
}

/// Return the sole workspace directory permitted to implement one centralized HTTP trait.
fn technical_http_trait_owner(trait_name: &str) -> Option<&'static str> {
    match trait_name {
        "FromRequest" | "FromRequestParts" => Some("crates/andromeda-http-extraction/"),
        "Header" => Some("crates/andromeda-http-headers/"),
        "IntoResponse" => Some("crates/andromeda-http-response/"),
        _ => None,
    }
}

/// Whether one inline item is compiled only for tests.
fn item_is_test_only(item: &Item) -> bool {
    match item {
        Item::Const(item) => is_test_only(&item.attrs),
        Item::Enum(item) => is_test_only(&item.attrs),
        Item::ExternCrate(item) => is_test_only(&item.attrs),
        Item::Fn(item) => is_test_only(&item.attrs),
        Item::ForeignMod(item) => is_test_only(&item.attrs),
        Item::Impl(item) => is_test_only(&item.attrs),
        Item::Macro(item) => is_test_only(&item.attrs),
        Item::Mod(item) => is_test_only(&item.attrs),
        Item::Static(item) => is_test_only(&item.attrs),
        Item::Struct(item) => is_test_only(&item.attrs),
        Item::Trait(item) => is_test_only(&item.attrs),
        Item::TraitAlias(item) => is_test_only(&item.attrs),
        Item::Type(item) => is_test_only(&item.attrs),
        Item::Union(item) => is_test_only(&item.attrs),
        Item::Use(item) => is_test_only(&item.attrs),
        Item::Verbatim(_) => false,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use crate::architecture::ArchitectureVisitor;
    use syn::visit::Visit;

    fn violations(source: &str) -> Vec<String> {
        let syntax = syn::parse_file(source).unwrap_or_else(|error| unreachable!("fixture parses: {error}"));
        let mut visitor = ArchitectureVisitor {
            path: "fixture.rs",
            violations: Vec::new(),
        };
        visitor.visit_file(&syntax);
        visitor.violations
    }

    #[test]
    fn rejects_public_reexports() {
        let violations = violations("pub use crate::name::Value;");
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn accepts_defining_module_imports_and_short_paths() {
        let violations = violations("use crate::domain::Value; fn read() { Value::read(); }");
        assert!(violations.is_empty());
    }

    #[test]
    fn ignores_test_only_inline_items() {
        let violations = violations("#[cfg(test)] mod tests { pub use crate::name::Value; }");
        assert!(violations.is_empty());
    }

    #[test]
    fn rejects_centralized_http_trait_implementations_outside_their_owner() {
        let violations = violations("impl<S> axum::extract::FromRequestParts<S> for Value {}");
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn accepts_centralized_http_trait_implementations_inside_their_owner() {
        let syntax = syn::parse_file("impl axum::response::IntoResponse for Value {}").unwrap_or_else(|error| unreachable!("fixture parses: {error}"));
        let mut visitor = ArchitectureVisitor {
            path: "crates/andromeda-http-response/src/value.rs",
            violations: Vec::new(),
        };
        visitor.visit_file(&syntax);
        assert!(visitor.violations.is_empty());
    }
}
