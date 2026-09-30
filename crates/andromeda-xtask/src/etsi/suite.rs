//! The pinned official ETSI NGSI-LD test-suite checkout: cloning it once into
//! `target/` and resetting it to the exact commit the matrix was recorded against.
//! The broker URL and core @context are no longer rewritten here at runtime; the
//! infra patch set (`suite-delta/infra`) bakes them into the checkout instead.

use crate::{error::XtaskError, etsi};
use iri_rs::{Iri, iri};
use std::{fs, path::Path, process::Command};

/// Upstream origin of the official ETSI NGSI-LD test suite.
const SUITE_ORIGIN: Iri<&str> = iri!("https://forge.etsi.org/rep/cim/ngsi-ld-test-suite.git");
/// The commit the test-case matrix in `docs/official-ngsi-ld-tests.md` is recorded
/// against; every corrected-material patch is authored to apply at this revision.
pub(crate) const PINNED_COMMIT: &str = "48d5ff4e5c6f83527d64d0960703a25939521308";

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
    let status = Command::new("git").args(["clone", "--quiet", SUITE_ORIGIN.as_str()]).arg(checkout).status()?;
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
    if !reset.success() {
        return Err(XtaskError::SuiteReset);
    }
    // `reset --hard` restores tracked files but leaves untracked ones behind. A
    // corrected-material patch that adds a file leaves that file untracked, so an
    // interrupted run (whose `PatchGuard` never reverses its patches) would poison
    // the next run: re-applying the file-creating patch fails with "already exists
    // in working directory". Removing untracked files here makes provisioning
    // idempotent regardless of how a previous run ended.
    let cleaned = Command::new("git").arg("-C").arg(checkout).args(["clean", "-fdq"]).status()?;
    if cleaned.success() { Ok(()) } else { Err(XtaskError::SuiteReset) }
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

#[cfg(test)]
mod tests {
    use crate::etsi::suite::PINNED_COMMIT;

    #[test]
    fn pinned_commit_is_a_full_length_hexadecimal_sha1() {
        assert_eq!(PINNED_COMMIT.len(), 40);
        assert!(PINNED_COMMIT.chars().all(|character| character.is_ascii_hexdigit()));
    }
}
