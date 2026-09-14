//! Permanent source guards for broker-wide structural invariants.

use crate::source_scan::{item_is_test_only, rust_file_paths, workspace_sources};
use quote::ToTokens;
use std::{collections::BTreeSet, fs, path::Path};
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
/// Panics when a forbidden source shape is found, a retired path reappears, or
/// the broker configuration cannot be read.
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
        // No configuration file is shipped: the broker reads a default file from
        // the platform configuration directory, generated on demand.
        "config/andromeda.toml",
        "config",
    ];
    for path in retired_paths {
        if root.join(path).exists() {
            violations.push(format!("retired path exists: {path}"));
        }
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
/// umbrella crate name returns, or when a test source cannot be read or parsed.
pub fn assert_test_infrastructure_invariants(workspace_root: &str) {
    let root = Path::new(workspace_root);
    let mut violations = Vec::new();
    scan_test_sources(root, &mut violations);
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
///
/// A test source that cannot be read or parsed is reported as a violation of
/// its own: the guard cannot vouch for a file it never saw.
fn scan_test_sources(root: &Path, violations: &mut Vec<String>) {
    for path in rust_file_paths(&root.join("tests")) {
        let source = match fs::read_to_string(&path) {
            Ok(source) => source,
            Err(error) => {
                violations.push(format!("{} could not be read: {error}", path.display()));
                continue;
            }
        };
        let relative = path.strip_prefix(root).unwrap_or(&path).display();
        if source.contains(concat!("andromeda_test_", "common")) || source.contains(concat!("andromeda-test-", "common")) {
            violations.push(format!("{relative} contains the retired test-common name"));
        }
        if source.contains("TcpListener::bind") || source.contains("axum::serve") {
            violations.push(format!("{relative} binds a callback listener outside andromeda-test-mocks"));
        }
        let syntax = match syn::parse_file(&source) {
            Ok(syntax) => syntax,
            Err(error) => {
                violations.push(format!("{relative} does not parse as Rust: {error}"));
                continue;
            }
        };
        let mut visitor = TestInfrastructureVisitor {
            path: relative.to_string(),
            violations: Vec::new(),
        };
        visitor.visit_file(&syntax);
        violations.extend(visitor.violations);
    }
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

#[cfg(test)]
mod tests {
    use crate::architecture::ArchitectureVisitor;
    use syn::visit::Visit;

    fn violations(source: &str) -> Vec<String> {
        let syntax = syn::parse_file(source).unwrap_or_else(|error| panic!("fixture parses: {error}"));
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
        assert_eq!(violations, [] as [String; 0]);
    }

    #[test]
    fn ignores_test_only_inline_items() {
        let violations = violations("#[cfg(test)] mod tests { pub use crate::name::Value; }");
        assert_eq!(violations, [] as [String; 0]);
    }

    #[test]
    fn rejects_centralized_http_trait_implementations_outside_their_owner() {
        let violations = violations("impl<S> axum::extract::FromRequestParts<S> for Value {}");
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn accepts_centralized_http_trait_implementations_inside_their_owner() {
        let syntax = syn::parse_file("impl axum::response::IntoResponse for Value {}").unwrap_or_else(|error| panic!("fixture parses: {error}"));
        let mut visitor = ArchitectureVisitor {
            path: "crates/andromeda-http-response/src/value.rs",
            violations: Vec::new(),
        };
        visitor.visit_file(&syntax);
        assert_eq!(visitor.violations, [] as [String; 0]);
    }
}
