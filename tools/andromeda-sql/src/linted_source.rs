//! A file a lint finding is in: a migration, or a file of the formatter's
//! vocabulary.

use crate::{formatter_vocabulary::VocabularyFile, migration_file::MigrationFile, migration_version::MigrationVersion};

/// A file the lint reports findings in.
#[derive(Debug, Clone, PartialEq, Eq, derive_more::Display)]
pub enum LintedSource {
    /// A migration.
    #[display("{_0}")]
    Migration(MigrationFile),
    /// A vocabulary file of the formatter.
    #[display("{_0}")]
    Vocabulary(VocabularyFile),
}

impl LintedSource {
    /// The key the file is ordered and found by.
    #[must_use]
    pub const fn key(&self) -> SourceKey {
        match self {
            LintedSource::Migration(file) => SourceKey::Migration(file.version()),
            LintedSource::Vocabulary(file) => SourceKey::Vocabulary(*file),
        }
    }
}

/// What names a linted file: a migration by its version, a vocabulary file by
/// its kind. Migrations order first, in version order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SourceKey {
    /// The migration holding this version.
    Migration(MigrationVersion),
    /// This vocabulary file.
    Vocabulary(VocabularyFile),
}
