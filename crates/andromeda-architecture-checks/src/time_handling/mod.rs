//! The workspace rule that one module turns text into time values and back.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.6.3 pins the `DateTime`, `Date` and `Time`
//! forms completely, and the lexicon's `time` module is the one reading and the
//! one spelling of them: its reader admits exactly the clause's forms, its
//! renderer writes every instant the broker emits, and its resolution step
//! brings a broker-produced instant to the microsecond. A second reading
//! anywhere, `chrono`'s own RFC 3339 parser, a strftime template, a hand-written
//! regular expression of the forms, lets one path accept `+01:00` or a seventh
//! fractional digit that another refuses, and lets one response spell an
//! instant differently from the next.
//!
//! The guard reads every Rust source of the workspace, test code included,
//! because a test that parses with another reader asserts behaviour the broker
//! does not have. Outside the lexicon's `time` module it flags:
//!
//! - a name of `chrono`'s text conversions or sub-second rounding
//!   (`parse_from_rfc3339`, `parse_from_str`, `to_rfc3339`, `SecondsFormat`,
//!   `trunc_subsecs`, `FixedOffset`, ...);
//! - a `parse` whose turbofish or annotated binding names a `chrono` time type;
//! - a `format` call with a strftime template;
//! - a string literal spelling a strftime directive or a regular expression of
//!   a time value form; the PostgreSQL lift of stored time text may keep the
//!   `to_char` template it renders a time of day with, and nothing else;
//! - a manifest enabling `chrono`'s `serde` feature, which would let a model
//!   derive a reader for a bare `DateTime<Utc>` that skips the clause, or
//!   depending on another date-time library, which would bring a reader and a
//!   writer of its own;
//! - a name of, or a dependency on, the XSD datatype library outside the
//!   lexicon's datatype module, which reads XSD datatypes and never an NGSI-LD
//!   time value.
//!
//! The reading is syntactic, so it sees what a source names, not the types an
//! expression has: a `chrono` value written through its own `Display` or read
//! through an inferred `FromStr` is not caught by name. `chrono`'s `serde`
//! feature being off closes the serde half of that at compile time.

mod manifest_reading;
mod source_reading;
mod xsd_reading;

use crate::{
    dependencies::workspace_manifest_paths,
    source_scan::every_workspace_source,
    time_handling::{
        manifest_reading::date_time_library_findings,
        source_reading::time_handling_findings,
        xsd_reading::{xsd_dependency_findings, xsd_names},
    },
};
use std::{fs, path::Path};

/// The module that owns every conversion between text and a time value.
const TIME_OWNER: &str = "crates/andromeda-ngsi-ld-lexicon/src/time/";

/// The guard's own sources, which spell the names they look for.
const GUARD_SOURCES: &str = "crates/andromeda-architecture-checks/src/time_handling/";

/// The module that reads XSD datatypes, the one source naming the XSD library.
const XSD_OWNER: &str = "crates/andromeda-ngsi-ld-lexicon/src/datatype/";

/// The manifest of the crate holding [`XSD_OWNER`].
const XSD_OWNER_MANIFEST: &str = "crates/andromeda-ngsi-ld-lexicon/Cargo.toml";

/// The PostgreSQL lift of stored time text, which renders a time of day with `to_char`.
const SQL_LIFT_OWNER: &str = "crates/andromeda-postgresql-query/src/temporal_operand.rs";

/// One site that handles time values outside their owner.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TimeHandlingSite {
    /// Workspace-relative path of the file the site lives in.
    pub path: String,
    /// One-based line the site starts on; zero for a manifest entry.
    pub line: usize,
    /// What the site does.
    pub subject: String,
}

impl TimeHandlingSite {
    /// The single-line report for this site, `path:line: subject`.
    #[must_use]
    pub fn report(&self) -> String {
        format!("{}:{}: {}", self.path, self.line, self.subject)
    }
}

/// Every site in the workspace that handles time values outside their owner,
/// sorted by path and line.
///
/// A manifest that cannot be read or parsed is reported as a site of its own.
///
/// # Panics
///
/// Panics when a Rust source cannot be read or parsed.
#[must_use]
pub fn time_handling_sites(workspace_root: &str) -> Vec<TimeHandlingSite> {
    let root = Path::new(workspace_root);
    let mut sites = Vec::new();
    for source in every_workspace_source(root) {
        if source.workspace_path.starts_with(GUARD_SOURCES) || source.is_black_box_suite() {
            continue;
        }
        if !source.workspace_path.starts_with(XSD_OWNER) {
            sites.extend(xsd_names(&source.syntax).into_iter().map(|line| TimeHandlingSite {
                path: source.workspace_path.clone(), // clone: every site of the file reports its path
                line,
                subject: "names the XSD datatype library outside the lexicon's datatype module".to_owned(),
            }));
        }
        if source.workspace_path.starts_with(TIME_OWNER) {
            continue;
        }
        let keeps_sql_template = source.workspace_path == SQL_LIFT_OWNER;
        sites.extend(
            time_handling_findings(&source.syntax, keeps_sql_template)
                .into_iter()
                .map(|(line, subject)| TimeHandlingSite {
                    path: source.workspace_path.clone(), // clone: every site of the file reports its path
                    line,
                    subject,
                }),
        );
    }
    for manifest in workspace_manifest_paths(root) {
        let path = manifest.strip_prefix(root).unwrap_or(&manifest).to_string_lossy().replace('\\', "/");
        let findings = match fs::read_to_string(&manifest) {
            Ok(text) if path == XSD_OWNER_MANIFEST => date_time_library_findings(&text),
            Ok(text) => date_time_library_findings(&text).into_iter().chain(xsd_dependency_findings(&text)).collect(),
            Err(error) => vec![format!("could not be read: {error}")],
        };
        sites.extend(findings.into_iter().map(|subject| TimeHandlingSite {
            path: path.clone(), // clone: every finding of the manifest reports its path
            line: 0,
            subject,
        }));
    }
    sites.sort();
    sites
}

/// Assert that time values are read and written only by their owner.
///
/// # Panics
///
/// Panics on any site [`time_handling_sites`] reports.
pub fn assert_time_values_have_one_owner(workspace_root: &str) {
    let sites = time_handling_sites(workspace_root);
    assert!(
        sites.is_empty(),
        "time values are read or written outside {TIME_OWNER}; parse through `NgsiLdDateTime`, `NgsiLdDate`, or `NgsiLdTime`, write through `instant_text` or `time_of_day_text`, truncate through `to_microsecond`, and carry serde members through `instant_serde`:\n{}",
        sites.iter().map(TimeHandlingSite::report).collect::<Vec<_>>().join("\n")
    );
}
