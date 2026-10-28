//! Permanent source guards for broker-wide structural invariants.

use crate::source_scan::{item_is_test_only, workspace_sources};
use quote::ToTokens;
use std::{collections::BTreeSet, path::Path};
use syn::{
    Item,
    ItemImpl,
    ItemUse,
    Visibility,
    visit::{Visit, visit_ident, visit_item, visit_item_impl, visit_lit_str},
};

/// Assert the structural invariants production sources must keep.
///
/// The scan covers production Rust below `src/`, `crates/`, and `tools/`. It
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
#[expect(
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::architecture::ArchitectureVisitor;
    use pretty_assertions::assert_eq;
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
