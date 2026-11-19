//! The workspace rule that the meaning of a `q` term is read in the language
//! crate only.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.9 gives a Query Term one meaning wherever
//! it is evaluated, and every place that decides what a term means by matching
//! on its syntax tree is a reading of the clause that can drift from the
//! others. The guard flags, in any production source outside
//! `crates/andromeda-ngsi-ld-query/src/filter/`, a pattern that names a
//! variant of `QueryTerm`, `SetMembership`, `OrderingOperator`, `EqualityValue` or
//! `Scalar`: a `match` arm, an `if let`, a
//! `let` with `else`, and the pattern of a `matches!` invocation, nested in
//! another macro or not. A variant is named as `Type::Variant`, as
//! `Self::Variant` inside an `impl` of a watched type, through a `use` that
//! imports it by name, or through a rename of the type; a glob import of a
//! watched type's variants is a site of its own. Calling an accessor such as
//! `QueryTerm::path()` and building a variant stay free, since neither
//! decides what a term means.
//!
//! Every site fails the gate.

use crate::{
    q_reading::visitor::QReadingVisitor,
    source_scan::{is_build_script, workspace_sources},
};
use std::{collections::BTreeMap, path::Path};
use syn::{File, visit::Visit};

mod imports;
mod visitor;

/// The directory that owns the reading of a `q` term.
const OWNER: &str = "crates/andromeda-ngsi-ld-query/src/filter/";

/// Every production site outside the owner that matches on a `q` term's
/// syntax tree, as `path:line: what`, grouped by file.
#[must_use]
pub fn q_reading_sites(workspace_root: &str) -> BTreeMap<String, Vec<String>> {
    workspace_sources(Path::new(workspace_root))
        .into_iter()
        .filter(|source| is_scanned(&source.workspace_path))
        .map(|source| (source_sites(&source.workspace_path, &source.syntax), source.workspace_path))
        .filter(|(sites, _)| !sites.is_empty())
        .map(|(sites, path)| (path, sites))
        .collect()
}

/// Whether a source is production code outside the owner.
fn is_scanned(path: &str) -> bool {
    !path.starts_with(OWNER) && !path.contains("/tests/") && !path.ends_with("tests.rs") && !is_build_script(path)
}

/// The sites of one parsed source, as `path:line: what`.
fn source_sites(path: &str, syntax: &File) -> Vec<String> {
    let mut visitor = QReadingVisitor::for_file(syntax);
    visitor.visit_file(syntax);
    visitor.into_sites().into_iter().map(|(line, what)| format!("{path}:{line}: {what}")).collect()
}

/// Assert that no production source outside the owner reads a `q` term.
///
/// # Panics
///
/// Panics when a file outside the owner holds a site, or when the workspace
/// source tree cannot be read or parsed.
pub fn assert_q_reading_has_one_owner(workspace_root: &str) {
    let sites: Vec<String> = q_reading_sites(workspace_root).into_values().flatten().collect();
    assert!(
        sites.is_empty(),
        "a q term is read outside {OWNER}; decide its meaning in the language crate and read the result:\n{}",
        sites.join("\n")
    );
}

#[cfg(test)]
mod tests;
