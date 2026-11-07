//! The workspace rule that one engine owns clause 4.5.23 Linked Entity
//! retrieval.
//!
//! `andromeda-linked-retrieval` decides, for every consumer, whether a Linked
//! Entity reference is admitted, which Linked Entities are retrieved, which
//! clause 4.21 list narrows each of them, and where each is nested. Every copy
//! of that design so far drifted from the others, so the guard flags, in any
//! production source outside the engine, the marks a second copy leaves:
//!
//! - reading a nested projection term (`linked_terms`, `linked_depth`,
//!   `matching_term`), which only the clause 4.21 grammar may also do;
//! - matching a pattern on a join mode, which only the mode's definition may
//!   also do;
//! - writing the clause 4.5.23.2 `entity` or `entityList` member (passing it
//!   as the key of an `insert`, `entry`, `serialize_entry` or
//!   `serialize_field`), which only the two inline writers may also do;
//!   reading or re-spelling a member already nested stays free;
//! - building an `AttributeIriIndex` by hand (`new`, then `insert`), which
//!   only the index itself does, expanding the keys it is handed;
//! - building one of the admission `ParameterError` variants anywhere but
//!   their `From<AdmissionError>` conversion;
//! - implementing `LinkedSource` or `LinkedRepresentation` in a file not
//!   registered as an adapter;
//! - naming a type or function after a private walk (`JoinPath`, `Ancestor`,
//!   `LinkedCandidate`, `resolve_linked`, `traverse`).

use crate::{
    linked_retrieval::{rule::ENGINE, visitor::LinkedRetrievalVisitor},
    source_scan::workspace_sources,
};
use std::{collections::BTreeSet, path::Path};
use syn::{File, visit::Visit};

mod rule;
mod visitor;

/// Every production site that decides part of Linked Entity retrieval outside
/// the engine, as `path: what`, sorted.
#[must_use]
pub fn linked_retrieval_violations(workspace_root: &str) -> Vec<String> {
    workspace_sources(Path::new(workspace_root))
        .into_iter()
        .filter(|source| !source.workspace_path.contains("/tests/") && !source.workspace_path.ends_with("build.rs"))
        .flat_map(|source| {
            source_violations(&source.workspace_path, &source.syntax)
                .into_iter()
                .map(move |violation| format!("{}: {violation}", source.workspace_path))
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Assert that Linked Entity retrieval is decided by its engine only.
///
/// # Panics
///
/// Panics on any site [`linked_retrieval_violations`] reports, or when the
/// workspace source tree cannot be read or parsed.
pub fn assert_linked_retrieval_has_one_owner(workspace_root: &str) {
    let violations = linked_retrieval_violations(workspace_root);
    assert!(
        violations.is_empty(),
        "Linked Entity retrieval is decided outside {ENGINE}; change the engine so every consumer receives the change:\n{}",
        violations.join("\n")
    );
}

/// The violations of the source at `path`, in the order they occur.
fn source_violations(path: &str, syntax: &File) -> Vec<String> {
    let mut visitor = LinkedRetrievalVisitor::default();
    visitor.visit_file(syntax);
    visitor
        .into_findings()
        .into_iter()
        .filter(|(rule, _)| !rule.permits(path))
        .map(|(_, violation)| violation)
        .collect()
}

#[cfg(test)]
mod tests;
