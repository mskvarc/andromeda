//! An Entity type unique to one test run.
//!
//! A scenario that registers a Context Source and then fails leaves that
//! registration behind, and clause 5.12 selects registrations by the Entity
//! type they cover. A scenario whose expected answer depends on exactly which
//! registrations matched -- how many sources hold the data, or which versions
//! of an Entity are received -- is therefore not repeatable while it shares an
//! Entity type with its own previous runs.

use andromeda_test_harness::environment::TestEnvironment;

/// An Entity type built from `base` and this run's own isolation prefix.
///
/// The prefix is the token the harness already isolates every identifier
/// under, so the derived type is unique to the run without needing a second
/// source of uniqueness. Only the trailing characters are taken, and only the
/// alphanumeric ones, so the result is a short valid term.
#[must_use]
pub fn probe_entity_type(scope: &TestEnvironment, base: &str) -> String {
    let unique: String = scope.id_prefix().chars().filter(char::is_ascii_alphanumeric).rev().take(12).collect();
    format!("{base}{unique}")
}
