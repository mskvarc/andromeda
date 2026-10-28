//! `andromeda-sql verify`: hold the migrations to the seal ledger, and with
//! `--against` the ledger to the one an earlier revision holds.
//!
//! The check against a revision is what stops a sealed migration being edited
//! and sealed again: the working copy's ledger would agree with its files, but
//! the section of the release would differ from the one the revision holds.
//! With `--release`, the tree is also held to what tagging that release needs.

use crate::{
    git_revision::{GitRevision, GitRevisionError},
    migration_file::MigrationFile,
    migration_tree::{MigrationTree, MigrationTreeError},
    release_tag::ReleaseTag,
    seal_ledger::{LEDGER_PATH, SealLedger, SealLedgerError},
    seal_verification::{changed_since, release_violations, violations},
    seal_violation::SealViolation,
};
use std::{fs, io, path::Path};

/// Every way the migrations below `root` and the seal ledger break the seal,
/// with `against` every section the ledger at that revision holds that the
/// working copy no longer holds unchanged, and with `release` every way the
/// tree falls short of what tagging that release needs.
///
/// # Errors
///
/// Returns [`VerifyError`] when the migrations, the ledger or the revision
/// cannot be read.
pub fn verify_migrations(root: &Path, against: Option<&GitRevision>, release: Option<&ReleaseTag>) -> Result<Vec<SealViolation>, VerifyError> {
    let text = SealLedger::read_text(root)?;
    let ledger = text.as_deref().map_or_else(|| Ok(SealLedger::default()), SealLedger::parse)?;
    let migrations = migration_bytes(root)?;
    let mut found = violations(text.as_deref(), &ledger, &migrations)?;
    if let Some(release) = release {
        found.extend(release_violations(&ledger, &migrations, release));
    }
    if let Some(revision) = against
        && let Some(earlier) = revision.read_file(root, Path::new(LEDGER_PATH))?
    {
        let earlier = SealLedger::parse(&earlier).map_err(|source| VerifyError::EarlierLedger {
            revision: revision.clone(),
            source,
        })?;
        found.extend(changed_since(&ledger, &earlier, revision));
    }
    Ok(found)
}

/// Every migration below `root`, in version order, with its raw bytes.
///
/// # Errors
///
/// Returns [`VerifyError`] when the migrations cannot be listed or one cannot
/// be read.
pub fn migration_bytes(root: &Path) -> Result<Vec<(MigrationFile, Vec<u8>)>, VerifyError> {
    MigrationTree::read(root)?
        .into_files()
        .map(|file| match fs::read(file.path()) {
            Ok(bytes) => Ok((file, bytes)),
            Err(source) => Err(VerifyError::Read { file: Box::new(file), source }),
        })
        .collect()
}

/// Why the seal could not be verified.
#[derive(Debug, thiserror::Error)]
pub enum VerifyError {
    /// The migrations could not be listed.
    #[error(transparent)]
    Tree(#[from] MigrationTreeError),
    /// A migration could not be read.
    #[error("{file} could not be read: {source}")]
    Read {
        /// The migration concerned.
        file: Box<MigrationFile>,
        /// What reading it failed with.
        source: io::Error,
    },
    /// The ledger could not be read.
    #[error(transparent)]
    Ledger(#[from] SealLedgerError),
    /// The ledger could not be read at the revision.
    #[error(transparent)]
    Revision(#[from] GitRevisionError),
    /// The ledger at the revision is not a seal ledger.
    #[error("{LEDGER_PATH} at {revision} could not be read: {source}")]
    EarlierLedger {
        /// The revision concerned.
        revision: GitRevision,
        /// Why its ledger could not be read.
        source: SealLedgerError,
    },
}
