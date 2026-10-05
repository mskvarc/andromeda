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
//! hand JSON to and from the processor. Imports count once per leaf, and a
//! local name bound by one counts wherever it is used, inside macro calls too.
//!
//! Detection is deny-by-default over a per-file baseline of counts that can
//! only shrink: a file not on the baseline must name no tree, a baselined file
//! may not exceed its count, and a baselined file that dropped below its count
//! must have its entry lowered (or removed at zero). Integration tests, build
//! scripts, `*tests.rs` files, the test-infrastructure crates and test-only
//! items are not production paths and are not inspected; tests keep `json!` to
//! build fixtures and to compare parsed bodies.

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
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

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

/// Where the sites of a workspace depart from the baseline.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct BaselineDrift {
    /// Files naming more trees than their baseline allows, as `path: count
    /// (baseline n)`, each followed by its sites.
    pub grown: Vec<String>,
    /// Baselined files naming fewer trees than recorded, as `path: count
    /// (baseline n)`; the entry is lowered to the count, or removed at zero.
    pub shrunk: Vec<String>,
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
            !source.workspace_path.contains("/tests/") && !source.workspace_path.ends_with("build.rs") && !source.workspace_path.ends_with("tests.rs")
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

/// Compare the sites found against the per-file baseline counts.
#[must_use]
pub fn baseline_drift(sites: &[UntypedJsonSite], baseline: &[(&str, usize)]) -> BaselineDrift {
    let mut by_path: BTreeMap<&str, Vec<&UntypedJsonSite>> = BTreeMap::new();
    for site in sites {
        by_path.entry(site.path.as_str()).or_default().push(site);
    }
    let allowed: BTreeMap<&str, usize> = baseline.iter().copied().collect();
    let paths: BTreeSet<&str> = by_path.keys().copied().chain(allowed.keys().copied()).collect();

    let mut drift = BaselineDrift::default();
    for path in paths {
        let found = by_path.get(path).map_or(&[][..], Vec::as_slice);
        let count = found.len();
        let recorded = allowed.get(path).copied().unwrap_or(0);
        if count > recorded {
            let reports = found.iter().map(|site| format!("  {}", site.report())).collect::<Vec<_>>().join("\n");
            drift.grown.push(format!("{path}: {count} (baseline {recorded})\n{reports}"));
        } else if count < recorded {
            drift.shrunk.push(format!("{path}: {count} (baseline {recorded})"));
        }
    }
    drift
}

/// Assert that no production source names an untyped JSON tree beyond the
/// baseline, and that the baseline records the current counts exactly.
///
/// # Panics
///
/// Panics on a file over its baseline count (or naming a tree while off the
/// baseline), on a baselined file now under its count, or when the workspace
/// source tree cannot be read or parsed.
pub fn assert_no_untyped_json(workspace_root: &str, baseline: &[(&str, usize)]) {
    let drift = baseline_drift(&untyped_json_sites(workspace_root), baseline);
    assert!(
        drift.grown.is_empty(),
        "these files name an untyped JSON tree beyond the baseline; decode bytes into the typed model, write the model to bytes, or hold opaque JSON as a JsonLiteral:\n{}",
        drift.grown.join("\n")
    );
    assert!(
        drift.shrunk.is_empty(),
        "these files name fewer untyped JSON trees than the baseline records; lower each entry to its count (remove it at 0):\n{}",
        drift.shrunk.join("\n")
    );
}

#[cfg(test)]
mod tests;
