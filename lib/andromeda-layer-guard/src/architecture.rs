//! Permanent source guards for broker-wide structural invariants.

use crate::source_scan::{is_test_only, workspace_sources};
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
        "src/http/handler/types.rs",
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
        if let Some((_, trait_path, _)) = &item.trait_
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
