//! Where a declared object is written.

use crate::migration_version::MigrationVersion;

/// The migration an object is declared in, and the byte offset of its
/// declaration there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Origin {
    /// The migration, named by the version it alone holds.
    version: MigrationVersion,
    /// The byte offset within the migration's text.
    offset: usize,
}

impl Origin {
    /// Byte `offset` of the migration at `version`.
    #[must_use]
    pub const fn new(version: MigrationVersion, offset: usize) -> Origin {
        Origin { version, offset }
    }

    /// The migration, named by the version it alone holds.
    #[must_use]
    pub const fn version(&self) -> MigrationVersion {
        self.version
    }

    /// The byte offset within the migration's text.
    #[must_use]
    pub const fn offset(&self) -> usize {
        self.offset
    }
}
