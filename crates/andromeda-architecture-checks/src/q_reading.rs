//! The workspace rule that the meaning of a `q` term is read in the language
//! crate only.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.9 gives a Query Term one meaning wherever
//! it is evaluated, and every place that decides what a term means by matching
//! on its syntax tree is a reading of the clause that can drift from the
//! others. The guard flags, in any production source outside
//! `crates/andromeda-ngsi-ld-query/src/filter/`, a pattern that names a
//! variant of `Predicate`, `SetMembership`, `OrderingOp`, `CompareValue`,
//! `Scalar`, `CmpOp` or `PatternPolarity`: a `match` arm, an `if let`, a
//! `let` with `else`, and the pattern of a `matches!` invocation, nested in
//! another macro or not. A variant is named as `Type::Variant`, as
//! `Self::Variant` inside an `impl` of a watched type, through a `use` that
//! imports it by name, or through a rename of the type; a glob import of a
//! watched type's variants is a site of its own. Calling an accessor such as
//! `Predicate::path()` and building a variant stay free, since neither
//! decides what a term means.
//!
//! The baseline counts, per file, the sites the in-memory evaluator and the
//! SQL renderings hold. A file may hold no more sites than its count, and a
//! file that holds fewer must lower its count, so the baseline only shrinks.

use crate::{q_reading::visitor::QReadingVisitor, source_scan::workspace_sources};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
use syn::{File, visit::Visit};

mod imports;
mod visitor;

/// The directory that owns the reading of a `q` term.
const OWNER: &str = "crates/andromeda-ngsi-ld-query/src/filter/";

/// The sites outside the owner, as the file and the number of sites it
/// holds. Each change that moves a reading into the owner lowers or removes
/// its file's entry.
pub const Q_READING_BASELINE: &[(&str, usize)] = &[
    ("crates/andromeda-ngsi-ld-query-evaluation/src/q/eval.rs", 26),
    ("crates/andromeda-postgresql-context-query/src/csr/filter.rs", 19),
    ("crates/andromeda-postgresql-query/src/jsonb.rs", 7),
    ("crates/andromeda-postgresql-query/src/operator.rs", 8),
    ("crates/andromeda-postgresql-query/src/q_predicate/equality_term.rs", 9),
    ("crates/andromeda-postgresql-query/src/q_predicate/json_value_term.rs", 3),
    ("crates/andromeda-postgresql-query/src/q_predicate/keyed_existence.rs", 1),
    ("crates/andromeda-postgresql-query/src/q_predicate/keyed_hop.rs", 8),
    ("crates/andromeda-postgresql-query/src/q_predicate/keyed_ordering.rs", 16),
    ("crates/andromeda-postgresql-query/src/q_predicate/keyed_pattern.rs", 1),
    ("crates/andromeda-postgresql-query/src/q_predicate/keyed_term.rs", 5),
    ("crates/andromeda-postgresql-query/src/q_predicate/language_term.rs", 24),
    ("crates/andromeda-postgresql-query/src/q_predicate/projection.rs", 5),
    ("crates/andromeda-postgresql-query/src/q_predicate/string_ordering.rs", 14),
    ("crates/andromeda-postgresql-query/src/q_predicate/structured_target.rs", 7),
    ("crates/andromeda-postgresql-query/src/q_predicate/sub_attribute/keyed_equality.rs", 3),
    ("crates/andromeda-postgresql-query/src/q_predicate/sub_attribute/member_field.rs", 51),
    ("crates/andromeda-postgresql-query/src/q_predicate/typed_column.rs", 25),
    ("crates/andromeda-postgresql-query/src/q_predicate/value_object_member.rs", 10),
    ("crates/andromeda-postgresql-query/src/sub_attribute_key/operand_keys.rs", 7),
];

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
    !path.starts_with(OWNER) && !path.contains("/tests/") && !path.ends_with("tests.rs") && !path.ends_with("build.rs")
}

/// The sites of one parsed source, as `path:line: what`.
fn source_sites(path: &str, syntax: &File) -> Vec<String> {
    let mut visitor = QReadingVisitor::for_file(syntax);
    visitor.visit_file(syntax);
    visitor.into_sites().into_iter().map(|(line, what)| format!("{path}:{line}: {what}")).collect()
}

/// How the sites found compare with the baseline.
#[derive(Debug, Default, Eq, PartialEq)]
pub struct BaselineComparison {
    /// The sites of every file holding more sites than its baseline count.
    pub unexpected: Vec<String>,
    /// Every baseline entry whose file holds fewer sites than its count, with
    /// the count it holds now.
    pub stale: Vec<String>,
}

/// Compare `sites` with `baseline`.
#[must_use]
pub fn compare_with_baseline(sites: &BTreeMap<String, Vec<String>>, baseline: &[(&str, usize)]) -> BaselineComparison {
    let counted: BTreeMap<&str, usize> = baseline.iter().copied().collect();
    let unexpected = sites
        .iter()
        .filter(|(path, found)| found.len() > counted.get(path.as_str()).copied().unwrap_or(0))
        .flat_map(|(_, found)| found.iter().cloned())
        .collect();
    let stale = counted
        .iter()
        .filter_map(|(path, count)| {
            let held = sites.get(*path).map_or(0, Vec::len);
            (held < *count).then(|| format!("{path}: {held} sites, baseline {count}"))
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    BaselineComparison { unexpected, stale }
}

/// Assert that no production source outside the owner reads a `q` term
/// beyond `baseline`.
///
/// # Panics
///
/// Panics when a file holds more sites than its baseline count, when a
/// baseline count is higher than the sites its file holds, or when the
/// workspace source tree cannot be read or parsed.
pub fn assert_q_reading_has_one_owner(workspace_root: &str, baseline: &[(&str, usize)]) {
    let comparison = compare_with_baseline(&q_reading_sites(workspace_root), baseline);
    assert!(
        comparison.unexpected.is_empty(),
        "a q term is read outside {OWNER}; decide its meaning in the language crate and read the result:\n{}",
        comparison.unexpected.join("\n")
    );
    assert!(
        comparison.stale.is_empty(),
        "these files read fewer q terms than the baseline counts; lower their entries:\n{}",
        comparison.stale.join("\n")
    );
}

#[cfg(test)]
mod tests;
