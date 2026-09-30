//! The workspace-wide ban on string-typed error values.
//!
//! An error is a typed value: a `thiserror` enum whose variants carry typed
//! fields and chain their cause through `#[source]`/`#[from]`. A message built
//! with `format!` or a source error flattened through `to_string()` throws the
//! structure away the same way a `dyn Error` does in
//! [`crate::type_erased_error`]: a caller can no longer match on what went
//! wrong, and the source chain ends at the string. Text an error shows a human
//! belongs in its `#[error("...")]` attribute, rendered from typed fields.
//!
//! The guard reads the syntax tree and fails on three shapes:
//!
//! - an error type (one deriving `Error` or implementing it in the same file)
//!   with a field that holds a `String` or `str`, alone or inside a container;
//! - a `Result` whose error type is a `String` or `str`;
//! - a string produced where an error value is expected: `Err(format!(..))`,
//!   `.map_err(|error| error.to_string())`, `.ok_or("..")`, or a string handed
//!   to a foreign error constructor such as `serde::de::Error::custom`.
//!
//! The ban is total: there is no baseline. Integration tests, build scripts, and
//! test-only items are not production paths and are not inspected.

mod string_shape;
mod visitor;

use crate::{source_scan::workspace_sources, string_typed_error::visitor::StringTypedErrorVisitor};
use std::path::Path;
use strum::IntoStaticStr;

/// The shape in which a string stands in for a typed error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum StringTypedErrorKind {
    /// A field of an error type holds a `String` or `str`.
    ErrorField,
    /// A `Result` names a `String` or `str` as its error type.
    StringErrorResult,
    /// A string value is produced where an error value is expected.
    StringErrorValue,
}

/// One production site where a string stands in for a typed error.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct StringTypedErrorSite {
    /// Workspace-relative path of the file the site lives in.
    pub path: String,
    /// One-based line the site starts on.
    pub line: usize,
    /// Which shape the site takes.
    pub kind: StringTypedErrorKind,
    /// What the site names: the offending field, type, or call.
    pub subject: String,
}

impl StringTypedErrorSite {
    /// The single-line report for this site, `path:line: kind: subject`.
    #[must_use]
    pub fn report(&self) -> String {
        let kind: &'static str = self.kind.into();
        format!("{}:{}: {kind}: {}", self.path, self.line, self.subject)
    }
}

/// Every production site where a string stands in for a typed error, sorted.
///
/// # Panics
///
/// Panics when the workspace source tree cannot be read or parsed.
#[must_use]
pub fn string_typed_error_sites(workspace_root: &str) -> Vec<StringTypedErrorSite> {
    let mut sites = Vec::new();
    for source in workspace_sources(Path::new(workspace_root)) {
        if source.workspace_path.contains("/tests/") || source.workspace_path.ends_with("build.rs") {
            continue;
        }
        sites.extend(StringTypedErrorVisitor::inspect(&source.workspace_path, &source.syntax));
    }
    sites.sort();
    sites
}

/// Assert that no production source lets a string stand in for a typed error.
///
/// # Panics
///
/// Panics on any string-typed error site, or when the workspace source tree
/// cannot be read or parsed.
pub fn assert_no_string_typed_errors(workspace_root: &str) {
    let sites = string_typed_error_sites(workspace_root);
    assert!(
        sites.is_empty(),
        "{} string-typed error sites; give each failure a typed variant and chain its cause with #[source]:\n{}",
        sites.len(),
        sites.iter().map(StringTypedErrorSite::report).collect::<Vec<_>>().join("\n")
    );
}
