//! The workspace-wide ban on type-erased error values.
//!
//! The workspace uses typed error enums (`thiserror`) everywhere; a `dyn Error`
//! trait object — `Box<dyn Error>`, `&dyn Error`, `Box<dyn Error + Send + Sync>`
//! — throws that type information away. `anyhow::Error` is the same erasure by
//! another name and is barred at the manifest boundary in
//! [`crate::dependencies`]. This guard catches the trait-object form directly in
//! the syntax tree, so a `use`-aliased or fully-qualified spelling cannot slip
//! past a text match.
//!
//! Detection is deny-by-default over a shrinking baseline, like the raw-SQL
//! gate: the sites present when the gate was introduced are tolerated, a new one
//! anywhere else fails immediately, and a baseline site that stops erasing its
//! error fails too, so the list can only shrink.

use crate::source_scan::{item_is_test_only, workspace_sources};
use std::{collections::BTreeSet, path::Path};
use syn::{
    Item,
    TypeParamBound,
    TypeTraitObject,
    visit::{Visit, visit_item},
};

/// The production sites that name a `dyn …Error` trait object.
///
/// The one entry is forced by an external contract the workspace does not own:
/// the `reqwest` DNS `Resolve` trait hands resolution errors back as
/// `Box<dyn Error + Send + Sync>`, which cannot be a typed enum without
/// abandoning the upstream trait, so it is tolerated here rather than papered
/// over with an `#[allow]`.
pub const TYPE_ERASED_ERROR_BASELINE: &[&str] = &["crates/andromeda-outbound-http/src/resolver.rs"];

/// Every production source that names a `dyn …Error` trait object, sorted.
///
/// Integration tests under `tests/` and build scripts are not production paths;
/// test-only items inside a production file are skipped as the visitor descends.
#[must_use]
pub fn type_erased_error_sites(workspace_root: &str) -> Vec<String> {
    let mut sites = BTreeSet::new();
    for source in workspace_sources(Path::new(workspace_root)) {
        if source.workspace_path.contains("/tests/") || source.workspace_path.ends_with("build.rs") {
            continue;
        }
        let mut visitor = TypeErasedErrorVisitor { erased: false };
        visitor.visit_file(&source.syntax);
        if visitor.erased {
            sites.insert(source.workspace_path.clone());
        }
    }
    sites.into_iter().collect()
}

/// Assert that no unsanctioned, non-baseline production source erases an error.
///
/// # Panics
///
/// Panics on a new `dyn …Error` site, on a baseline site that no longer erases
/// an error, or when the workspace source tree cannot be read or parsed.
pub fn assert_no_type_erased_errors(workspace_root: &str, baseline: &[&str]) {
    let sites: BTreeSet<String> = type_erased_error_sites(workspace_root).into_iter().collect();
    let baseline: BTreeSet<&str> = baseline.iter().copied().collect();

    let unexpected: Vec<&str> = sites.iter().map(String::as_str).filter(|path| !baseline.contains(path)).collect();
    let stale: Vec<&str> = baseline.iter().copied().filter(|path| !sites.contains(*path)).collect();

    assert!(
        unexpected.is_empty(),
        "type-erased error values outside the sanctioned baseline; use a typed error enum:\n{}",
        unexpected.join("\n")
    );
    assert!(
        stale.is_empty(),
        "these baseline files no longer erase an error; delete them from the baseline:\n{}",
        stale.join("\n")
    );
}

/// Records whether one parsed source names a `dyn …Error` trait object outside
/// its test-only items.
struct TypeErasedErrorVisitor {
    /// Whether a `dyn …Error` trait object was seen.
    erased: bool,
}

impl<'ast> Visit<'ast> for TypeErasedErrorVisitor {
    fn visit_item(&mut self, item: &'ast Item) {
        if item_is_test_only(item) {
            return;
        }
        visit_item(self, item);
    }

    fn visit_type_trait_object(&mut self, object: &'ast TypeTraitObject) {
        if object.bounds.iter().any(bound_is_error_trait) {
            self.erased = true;
        }
    }
}

/// Whether a trait-object bound is the `Error` trait, under any path spelling.
///
/// The last path segment is the trait name, so `Error`, `std::error::Error`, and
/// a `use`-aliased `error::Error` all match, while a `Send`/`Sync`/lifetime
/// bound alongside it does not.
fn bound_is_error_trait(bound: &TypeParamBound) -> bool {
    let TypeParamBound::Trait(trait_bound) = bound else {
        return false;
    };
    trait_bound.path.segments.last().is_some_and(|segment| segment.ident == "Error")
}

#[cfg(test)]
mod tests {
    use crate::type_erased_error::TypeErasedErrorVisitor;
    use syn::visit::Visit;

    fn erases(source: &str) -> bool {
        let syntax = syn::parse_file(source).unwrap_or_else(|error| panic!("fixture parses: {error}"));
        let mut visitor = TypeErasedErrorVisitor { erased: false };
        visitor.visit_file(&syntax);
        visitor.erased
    }

    #[test]
    fn a_boxed_dyn_error_return_is_erased() {
        assert!(erases("fn run() -> Result<(), Box<dyn Error>> { Ok(()) }"));
    }

    #[test]
    fn a_qualified_and_bounded_dyn_error_is_erased() {
        assert!(erases("type R = Result<A, Box<dyn std::error::Error + Send + Sync>>;"));
        assert!(erases("fn walk(e: &(dyn Error + 'static)) {}"));
    }

    #[test]
    fn a_typed_error_enum_is_not_erased() {
        assert!(!erases("fn run() -> Result<(), ParseError> { Ok(()) }"));
    }

    #[test]
    fn a_non_error_trait_object_is_not_erased() {
        assert!(!erases("fn stream() -> Box<dyn Iterator<Item = u8>> { todo!() }"));
    }

    #[test]
    fn a_test_only_boxed_error_is_ignored() {
        assert!(!erases("#[cfg(test)] mod tests { fn run() -> Result<(), Box<dyn Error>> { Ok(()) } }"));
    }
}
