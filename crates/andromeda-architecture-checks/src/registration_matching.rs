//! The workspace rule that the model crate owns every registration and
//! subscription matching decision.
//!
//! Whether a registration covers an entity (ETSI GS CIM 009 v1.9.1 clause
//! 5.12), whether its `scope`, `geoQ`, `csf` and temporal filters admit it,
//! whether it is still in force (Table 5.2.9-1), whether a subscription is
//! notified (clause 5.8.1.4) and which entities its selectors select (clause
//! 5.2.33) each have one reading in the model crate. Every copy of those
//! readings outside it so far drifted from the others, so the guard flags, in
//! any production source outside the engine, the marks a second copy leaves:
//!
//! - in a source handling registrations or subscriptions, reading an
//!   `id_pattern` field or calling `is_match`: a registered or selected
//!   identifier decided by hand;
//! - calling `admits` or `matches` on a `type_` field: a registered or
//!   selected type decided by hand;
//! - reading `coverage.scope`: a registration's Scope matched by hand;
//! - naming a `ContextSourceProperties` constructor: a second `csf` view of a
//!   registration;
//! - in a source handling registrations or subscriptions, comparing an
//!   `expires_at` with the current instant: liveness or standing decided by
//!   hand;
//! - comparing with `SubscriptionStatus::Active`: standing decided on the
//!   status alone;
//! - building an `IdentifierAxis` through `IdentifierAxis::of`: a selector's
//!   identifier axis read a second time.
//!
//! Each finding names its `path:line`. The sources that read these members
//! only to spell, persist or push a rule down into SQL are registered as
//! adapters of the one rule they need, each with its reason.

use crate::{
    registration_matching::{
        rule::{ENGINE, stale_adapters},
        visitor::{Finding, RegistrationMatchingVisitor},
    },
    source_scan::workspace_sources,
};
use std::path::Path;
use syn::{File, visit::Visit};

mod rule;
mod selector_bindings;
mod token_reading;
mod visitor;

/// One production site that decides a registration or subscription match
/// outside the engine.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RegistrationMatchingSite {
    /// Workspace-relative path of the file the site lives in.
    pub path: String,
    /// One-based line the site starts on.
    pub line: usize,
    /// What the site decides.
    pub subject: String,
}

impl RegistrationMatchingSite {
    /// The single-line report for this site, `path:line: subject`.
    #[must_use]
    pub fn report(&self) -> String {
        format!("{}:{}: {}", self.path, self.line, self.subject)
    }
}

/// What one scan of the workspace found.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RegistrationMatchingScan {
    /// Every production site that decides a match outside the engine, sorted
    /// by path and line.
    pub sites: Vec<RegistrationMatchingSite>,
    /// Every registered adapter that no longer reads what its rule guards,
    /// one line each.
    pub stale_adapters: Vec<String>,
}

/// Scan every production source for matching decisions outside the engine,
/// and every registered adapter for whether it is still one.
///
/// # Panics
///
/// Panics when the workspace source tree cannot be read or parsed.
#[must_use]
pub fn registration_matching_scan(workspace_root: &str) -> RegistrationMatchingScan {
    let mut sites = Vec::new();
    let mut adapted = Vec::new();
    for source in workspace_sources(Path::new(workspace_root)) {
        let path = source.workspace_path.as_str();
        if path.contains("/tests/") || path.ends_with("build.rs") || path.ends_with("tests.rs") {
            continue;
        }
        for finding in source_findings(&source.syntax) {
            if finding.rule.permits(path) {
                adapted.push((finding.rule, source.workspace_path.clone())); // clone: the pair outlives the parsed source
            } else {
                sites.push(site(path, finding));
            }
        }
    }
    sites.sort();
    RegistrationMatchingScan {
        sites,
        stale_adapters: stale_adapters(&adapted),
    }
}

/// Assert that registration and subscription matching is decided by the
/// engine only, and that every registered adapter still is one.
///
/// # Panics
///
/// Panics on any site or stale adapter [`registration_matching_scan`]
/// reports, or when the workspace source tree cannot be read or parsed.
pub fn assert_registration_matching_has_one_owner(workspace_root: &str) {
    let scan = registration_matching_scan(workspace_root);
    assert!(
        scan.sites.is_empty(),
        "registration or subscription matching is decided outside {}; call the engine, or register a source that only spells, persists or pushes the rule down as an adapter:\n{}",
        ENGINE.join(", "),
        scan.sites.iter().map(|site| format!("  {}", site.report())).collect::<Vec<_>>().join("\n")
    );
    assert!(
        scan.stale_adapters.is_empty(),
        "these adapters no longer read what their rule guards; remove their entries:\n{}",
        scan.stale_adapters.join("\n")
    );
}

/// The decisions of one parsed source, in source order, whether or not its
/// path permits them.
fn source_findings(syntax: &File) -> Vec<Finding> {
    let mut visitor = RegistrationMatchingVisitor::default();
    visitor.visit_file(syntax);
    visitor.into_findings()
}

/// The site of `finding` in the source at `path`.
fn site(path: &str, finding: Finding) -> RegistrationMatchingSite {
    RegistrationMatchingSite {
        path: path.to_owned(),
        line: finding.start.line,
        subject: finding.subject,
    }
}

#[cfg(test)]
#[expect(
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests;
