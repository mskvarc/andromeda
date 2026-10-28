//! `andromeda-sql seal <version>`: record in the ledger every migration the
//! release adds, with the SHA-256 of its raw bytes, after which those files
//! never change.
//!
//! A release is sealed only when the seal holds, the version follows the last
//! sealed one, the release adds a migration, and every migration is in its
//! canonical layout and passes the lint; the ledger is written last, so a
//! refused seal leaves it as it was.

use crate::{
    canonical_layout::CanonicalLayout,
    file_digest::FileDigest,
    format_command::{FormatError, FormatMode, format_migrations},
    lint_command::{LintError, LintOutcome, lint_migrations},
    migration_file::MigrationFile,
    release_version::ReleaseVersion,
    seal_ledger::{
        SealLedger,
        SealLedgerError,
        sealed_release::{SealedMigration, SealedRelease},
    },
    seal_violation::SealViolation,
    squawk::Squawk,
    verify_command::{VerifyError, migration_bytes, verify_migrations},
};
use std::path::Path;

/// What a seal run did.
#[derive(Debug)]
pub enum SealOutcome {
    /// The release is sealed with this section.
    Sealed(SealedRelease),
    /// The release is not sealed, for this reason; the ledger is unchanged.
    Refused(SealRefusal),
}

/// Why a release was not sealed.
#[derive(Debug)]
pub enum SealRefusal {
    /// The migrations or the ledger already break the seal.
    Broken(Vec<SealViolation>),
    /// The version does not follow the last sealed release.
    NotAfterLatest {
        /// The release asked for.
        release: ReleaseVersion,
        /// The last sealed release.
        latest: ReleaseVersion,
    },
    /// Every migration is sealed already.
    NothingToSeal(ReleaseVersion),
    /// These migrations are not in their canonical layout.
    NotFormatted(Vec<MigrationFile>),
    /// The lint finds something.
    Lint(LintOutcome),
}

/// Seal `release` over the migrations below `root`, checking their layout
/// with `layout` and linting them with `squawk`.
///
/// # Errors
///
/// Returns [`SealError`] when the migrations or the ledger cannot be read, the
/// layout or the lint cannot be run, or the ledger cannot be written.
pub fn seal_migrations(root: &Path, release: ReleaseVersion, layout: &CanonicalLayout, squawk: &Squawk) -> Result<SealOutcome, SealError> {
    let broken = verify_migrations(root, None, None)?;
    if !broken.is_empty() {
        return Ok(SealOutcome::Refused(SealRefusal::Broken(broken)));
    }
    let ledger = SealLedger::load(root)?;
    if let Some(latest) = ledger.latest_release()
        && &release <= latest.version()
    {
        return Ok(SealOutcome::Refused(SealRefusal::NotAfterLatest {
            release,
            // clone: the refusal names the release it follows
            latest: latest.version().clone(),
        }));
    }
    let added = migration_bytes(root)?
        .into_iter()
        .filter(|(file, _)| !ledger.seals(file))
        .map(|(file, bytes)| SealedMigration::new(&file, FileDigest::of(&bytes)))
        .collect::<Vec<_>>();
    if added.is_empty() {
        return Ok(SealOutcome::Refused(SealRefusal::NothingToSeal(release)));
    }
    let drifted = format_migrations(root, layout, FormatMode::Check)?;
    if !drifted.drifted().is_empty() {
        return Ok(SealOutcome::Refused(SealRefusal::NotFormatted(drifted.drifted().to_vec())));
    }
    let lint = lint_migrations(root, squawk, layout.pg_format())?;
    if !lint.files().is_empty() {
        return Ok(SealOutcome::Refused(SealRefusal::Lint(lint)));
    }
    let section = SealedRelease::new(release, added);
    // clone: the ledger keeps the section and the outcome reports it
    ledger.with_release(section.clone()).write(root)?;
    Ok(SealOutcome::Sealed(section))
}

/// Why a seal could not be run.
#[derive(Debug, thiserror::Error)]
pub enum SealError {
    /// The seal could not be verified.
    #[error(transparent)]
    Verify(#[from] VerifyError),
    /// The ledger could not be read or written.
    #[error(transparent)]
    Ledger(#[from] SealLedgerError),
    /// The layout could not be checked.
    #[error(transparent)]
    Format(#[from] FormatError),
    /// The lint could not be run.
    #[error(transparent)]
    Lint(#[from] LintError),
}
