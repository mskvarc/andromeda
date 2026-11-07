//! The workspace-wide ban on untyped JSON trees in production code.
//!
//! Production code holds JSON in three forms only: a typed model, the bytes it
//! is decoded from or written to, and a `JsonLiteral` for the JSON the
//! specification carries as opaque data (NGSI-LD clause 4.5.24, JSON-LD 1.1
//! `@json`). A `serde_json::Value` tree is none of these: it allocates per node
//! and per key, loses member order, and recurses on drop, clone and print, so
//! every deep document it holds is a stack hazard.
//!
//! The guard reads every production source and counts the names that build,
//! hold or convert such a tree: `serde_json::{Value, Map, json!, to_value,
//! from_value}` (also through `serde_json::value`), and the JSON processor's
//! own tree (`jstrict`, `jsonld_syntax::Value`) outside the two places that
//! hand JSON to and from the processor, and the `toml` crate's tree
//! (`toml::Value`, `toml::Table`, `toml::map`) outside the one configuration
//! value that holds operator data passed on verbatim. Imports count once per leaf, and a
//! local name bound by one counts wherever it is used, inside macro calls too.
//!
//! Detection is deny-by-default and runs at zero: any production site naming
//! a tree fails the gate with its `path:line`. Integration tests, build
//! scripts, `*tests.rs` files and test-only items are not production paths and
//! are not inspected; tests keep `json!` to build fixtures and to compare parsed
//! bodies.

mod forbidden_name;
mod module_imports;
mod source_reading;

use crate::{
    source_scan::workspace_sources,
    untyped_json::{
        module_imports::{ModuleImports, ModulePosition},
        source_reading::untyped_json_findings,
    },
};
use std::path::Path;

/// One production site that names an untyped JSON tree.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct UntypedJsonSite {
    /// Workspace-relative path of the file the site lives in.
    pub path: String,
    /// One-based line the name starts on.
    pub line: usize,
    /// The name as written, path segments joined by `::`.
    pub subject: String,
}

impl UntypedJsonSite {
    /// The single-line report for this site, `path:line: subject`.
    #[must_use]
    pub fn report(&self) -> String {
        format!("{}:{}: {}", self.path, self.line, self.subject)
    }
}

/// Every production site that names an untyped JSON tree, sorted by path and line.
///
/// # Panics
///
/// Panics when the workspace source tree cannot be read or parsed.
#[must_use]
pub fn untyped_json_sites(workspace_root: &str) -> Vec<UntypedJsonSite> {
    let sources: Vec<_> = workspace_sources(Path::new(workspace_root))
        .into_iter()
        .filter(|source| {
            !source.is_black_box_suite()
                && !source.workspace_path.contains("/tests/")
                && !source.workspace_path.ends_with("build.rs")
                && !source.workspace_path.ends_with("tests.rs")
        })
        .collect();
    let positioned: Vec<_> = sources
        .iter()
        .map(|source| (ModulePosition::of(&source.workspace_path), &source.syntax))
        .collect();
    let imports = ModuleImports::settle(&positioned);
    let mut sites: Vec<UntypedJsonSite> = sources
        .iter()
        .zip(&positioned)
        .flat_map(|(source, (position, _))| source_sites(&source.workspace_path, &source.syntax, &imports, position.as_ref()))
        .collect();
    sites.sort();
    sites
}

/// The untyped JSON sites of one parsed source at `path`, in source order,
/// with the names the modules it imports from bound in `imports`.
pub(crate) fn source_sites(path: &str, syntax: &syn::File, imports: &ModuleImports, position: Option<&ModulePosition>) -> Vec<UntypedJsonSite> {
    untyped_json_findings(syntax, &|segments| imports.resolve(position, segments))
        .into_iter()
        .filter(|finding| !finding.tree.admitted_in(path))
        .map(|finding| UntypedJsonSite {
            path: path.to_owned(),
            line: finding.line,
            subject: finding.subject,
        })
        .collect()
}

/// The failure message for `sites`, one `path:line: subject` line per site, or
/// `None` when no production source names an untyped JSON tree.
#[must_use]
pub fn violation_report(sites: &[UntypedJsonSite]) -> Option<String> {
    if sites.is_empty() {
        return None;
    }
    let reports = sites.iter().map(|site| format!("  {}", site.report())).collect::<Vec<_>>().join("\n");
    Some(format!(
        "these sites name an untyped JSON tree; decode bytes into the typed model, write the model to bytes, or hold opaque JSON as a JsonLiteral:\n{reports}"
    ))
}

/// Assert that no production source names an untyped JSON tree.
///
/// # Panics
///
/// Panics on any production site naming a tree, or when the workspace source
/// tree cannot be read or parsed.
pub fn assert_no_untyped_json(workspace_root: &str) {
    let report = violation_report(&untyped_json_sites(workspace_root));
    assert!(report.is_none(), "{}", report.unwrap_or_default());
}

#[cfg(test)]
mod tests;
