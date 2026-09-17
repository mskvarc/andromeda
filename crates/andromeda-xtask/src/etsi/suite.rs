//! The pinned official ETSI NGSI-LD test-suite checkout: cloning it once into
//! `target/`, resetting it to the exact commit the matrix was recorded against,
//! and rewriting its runtime configuration for the broker under test.

use crate::{error::XtaskError, etsi};
use std::{borrow::Cow, fs, path::Path, process::Command};

/// Upstream origin of the official ETSI NGSI-LD test suite.
const SUITE_ORIGIN: &str = "https://forge.etsi.org/rep/cim/ngsi-ld-test-suite.git";
/// The commit the test-case matrix in `docs/official-ngsi-ld-tests.md` is recorded
/// against; every corrected-material patch is authored to apply at this revision.
const PINNED_COMMIT: &str = "48d5ff4e5c6f83527d64d0960703a25939521308";
/// The unversioned core @context identifier the suite's `variables.py` must use, so
/// term expansion agrees with the broker's advertised core context.
const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context.jsonld";
/// The suite's runtime configuration module, relative to the checkout root.
const VARIABLES_FILE: &str = "resources/variables.py";

/// Ensure the checkout exists at the pinned commit with a pristine working tree.
///
/// A fresh clone is made when the directory is absent; otherwise the pinned commit
/// is fetched if it is missing and the tree is hard-reset to it, discarding any
/// patches a previous run left behind. The checkout lives under `target/`, so this
/// only ever touches a disposable build artifact.
///
/// # Errors
///
/// Returns a clone, fetch, or reset failure, or an I/O error creating the parent.
pub(crate) fn provision() -> Result<(), XtaskError> {
    let checkout = etsi::checkout_dir();
    if checkout.join(".git").is_dir() { reset(&checkout) } else { clone(&checkout) }
}

/// Clone the suite into the checkout directory, then reset it to the pinned commit.
fn clone(checkout: &Path) -> Result<(), XtaskError> {
    if let Some(parent) = checkout.parent() {
        fs::create_dir_all(parent)?;
    }
    let status = Command::new("git").args(["clone", "--quiet", SUITE_ORIGIN]).arg(checkout).status()?;
    if !status.success() {
        return Err(XtaskError::SuiteClone);
    }
    reset(checkout)
}

/// Hard-reset an existing checkout to the pinned commit, fetching it first if the
/// local repository does not already have it.
fn reset(checkout: &Path) -> Result<(), XtaskError> {
    if !commit_present(checkout) {
        let fetched = Command::new("git").arg("-C").arg(checkout).args(["fetch", "--quiet", "origin"]).status()?;
        if !fetched.success() {
            return Err(XtaskError::SuiteFetch);
        }
    }
    let reset = Command::new("git")
        .arg("-C")
        .arg(checkout)
        .args(["reset", "--hard", "--quiet", PINNED_COMMIT])
        .status()?;
    if reset.success() {
        Ok(())
    } else {
        Err(XtaskError::SuiteReset(PINNED_COMMIT.to_owned()))
    }
}

/// Whether the local repository already has the pinned commit as a commit object.
fn commit_present(checkout: &Path) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(checkout)
        .args(["cat-file", "-e", &format!("{PINNED_COMMIT}^{{commit}}")])
        .status()
        .is_ok_and(|status| status.success())
}

/// Point the suite's broker URLs at the freshly built broker and pin the
/// unversioned core @context. Every other setting is left at the upstream default,
/// matching the recorded baseline configuration.
///
/// # Errors
///
/// Returns an I/O error reading or writing the configuration module.
pub(crate) fn configure(port: u16) -> Result<(), XtaskError> {
    let path = etsi::checkout_dir().join(VARIABLES_FILE);
    let broker_url = format!("http://localhost:{port}");
    let original = fs::read_to_string(&path)?;
    let rewritten = original
        .lines()
        .map(|line| rewrite_line(line, &broker_url))
        .collect::<Vec<Cow<'_, str>>>()
        .join("\n");
    // Preserve the original's trailing-newline shape so the tracked file stays
    // byte-clean against the pinned commit apart from the intended edits.
    let rewritten = if original.ends_with('\n') { format!("{rewritten}\n") } else { rewritten };
    fs::write(&path, rewritten)?;
    Ok(())
}

/// Rewrite a single `variables.py` line, retargeting the broker URL and core
/// @context assignments and leaving every other line untouched.
fn rewrite_line<'line>(line: &'line str, broker_url: &str) -> Cow<'line, str> {
    if is_assignment(line, "url") || is_assignment(line, "temporal_api_url") {
        let name = if is_assignment(line, "url") { "url" } else { "temporal_api_url" };
        return Cow::Owned(format!("{name} = '{broker_url}'"));
    }
    if is_assignment(line, "core_context") {
        return Cow::Owned(format!("core_context = '{CORE_CONTEXT}'"));
    }
    Cow::Borrowed(line)
}

/// Whether a Python line is a top-level assignment to `name` (`name = ...`),
/// distinguishing `url` from `temporal_api_url` by requiring the `=` to follow.
fn is_assignment(line: &str, name: &str) -> bool {
    line.strip_prefix(name).is_some_and(|rest| rest.trim_start().starts_with('='))
}

#[cfg(test)]
mod tests {
    use crate::etsi::suite::{PINNED_COMMIT, is_assignment, rewrite_line};
    use std::borrow::Cow;

    #[test]
    fn pinned_commit_is_a_full_length_hexadecimal_sha1() {
        assert_eq!(PINNED_COMMIT.len(), 40);
        assert!(PINNED_COMMIT.chars().all(|character| character.is_ascii_hexdigit()));
    }

    #[test]
    fn url_assignment_is_retargeted_to_the_broker() {
        let rewritten = rewrite_line("url = 'http://localhost:8080'", "http://localhost:1056");
        assert_eq!(rewritten, Cow::Owned::<str>("url = 'http://localhost:1056'".to_owned()));
    }

    #[test]
    fn temporal_api_url_is_retargeted_and_not_confused_with_url() {
        let rewritten = rewrite_line("temporal_api_url = 'http://localhost:8080'", "http://localhost:1056");
        assert_eq!(rewritten, Cow::Owned::<str>("temporal_api_url = 'http://localhost:1056'".to_owned()));
    }

    #[test]
    fn core_context_is_pinned_to_the_unversioned_identifier() {
        let rewritten = rewrite_line(
            "core_context = 'https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.6.jsonld'",
            "http://localhost:1056",
        );
        assert_eq!(
            rewritten,
            Cow::Borrowed("core_context = 'https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context.jsonld'")
        );
    }

    #[test]
    fn an_unrelated_assignment_is_left_borrowed_and_unchanged() {
        let rewritten = rewrite_line("notification_server_port = 8085", "http://localhost:1056");
        assert_eq!(rewritten, Cow::Borrowed("notification_server_port = 8085"));
    }

    #[test]
    fn a_similarly_named_assignment_is_not_matched() {
        assert!(!is_assignment("urls = 'x'", "url"));
        assert!(is_assignment("url = 'x'", "url"));
    }
}
