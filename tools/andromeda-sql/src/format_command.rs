//! `andromeda-sql format`: bring every migration to its canonical layout, or
//! report the ones that are not.
//!
//! A sealed migration is passed over: its sealed bytes are the release's
//! history, so a later change of `pg_format` or of the layout never rewrites
//! it, and `--check` never reports it.

use crate::{
    canonical_layout::{CanonicalLayout, CanonicalLayoutError},
    migration_file::MigrationFile,
    migration_tree::{MigrationTree, MigrationTreeError},
    seal_ledger::{SealLedger, SealLedgerError},
};
use std::{fs, io, path::Path};

/// What the command does with a migration that is not canonical.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatMode {
    /// Rewrite it in its canonical layout.
    Write,
    /// Report it and write nothing.
    Check,
}

/// The migrations a run found not canonical, in version order.
///
/// In [`FormatMode::Write`] each has been rewritten; in [`FormatMode::Check`]
/// none has.
#[derive(Debug)]
pub struct FormatOutcome {
    /// The migrations whose text differed from their canonical layout.
    drifted: Vec<MigrationFile>,
}

impl FormatOutcome {
    /// The migrations whose text differed from their canonical layout.
    #[must_use]
    pub fn drifted(&self) -> &[MigrationFile] {
        &self.drifted
    }
}

/// Lay out every migration below `root/migrations` the seal ledger does not
/// hold.
///
/// Every file is laid out before any is written, so a run that fails on one
/// migration leaves all of them as they were.
///
/// # Errors
///
/// Returns [`FormatError`] naming the file concerned when the migrations or
/// the ledger cannot be read, one cannot be laid out, or a rewrite fails.
pub fn format_migrations(root: &Path, layout: &CanonicalLayout, mode: FormatMode) -> Result<FormatOutcome, FormatError> {
    let ledger = SealLedger::load(root)?;
    let mut rewrites = Vec::new();
    for file in MigrationTree::read(root)?.into_files().filter(|file| !ledger.seals(file)) {
        let written = match fs::read_to_string(file.path()) {
            Ok(written) => written,
            Err(source) => return Err(FormatError::Read { file: Box::new(file), source }),
        };
        let canonical = match layout.of(&written) {
            Ok(canonical) => canonical,
            Err(source) => return Err(FormatError::Layout { file: Box::new(file), source }),
        };
        if canonical != written {
            rewrites.push((file, canonical));
        }
    }
    if mode == FormatMode::Write
        && let Some((index, source)) = rewrites
            .iter()
            .enumerate()
            .find_map(|(index, (file, canonical))| fs::write(file.path(), canonical).err().map(|source| (index, source)))
    {
        let (file, _) = rewrites.swap_remove(index);
        return Err(FormatError::Write { file: Box::new(file), source });
    }
    Ok(FormatOutcome {
        drifted: rewrites.into_iter().map(|(file, _)| file).collect(),
    })
}

/// Why the migrations could not be formatted.
#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    /// The migrations could not be read.
    #[error(transparent)]
    Tree(#[from] MigrationTreeError),
    /// The seal ledger could not be read.
    #[error(transparent)]
    Ledger(#[from] SealLedgerError),
    /// A migration could not be read.
    #[error("{file} could not be read: {source}")]
    Read {
        /// The migration concerned.
        file: Box<MigrationFile>,
        /// What reading it failed with.
        source: io::Error,
    },
    /// A migration could not be laid out.
    #[error("{file} could not be formatted: {source}")]
    Layout {
        /// The migration concerned.
        file: Box<MigrationFile>,
        /// Why it could not be laid out.
        source: CanonicalLayoutError,
    },
    /// A migration could not be rewritten.
    #[error("{file} could not be written: {source}")]
    Write {
        /// The migration concerned.
        file: Box<MigrationFile>,
        /// What writing it failed with.
        source: io::Error,
    },
}
