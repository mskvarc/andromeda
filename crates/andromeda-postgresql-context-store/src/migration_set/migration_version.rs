//! The version a migration file is numbered with.

/// The version of one migration: the number its file name starts with, unique
/// across every migration directory, so it orders every set the file belongs
/// to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, derive_more::Display)]
#[display("{_0}")]
pub struct MigrationVersion(i64);

impl MigrationVersion {
    /// The version numbered `version`, as sqlx numbers a migration.
    #[must_use]
    pub const fn new(version: i64) -> MigrationVersion {
        MigrationVersion(version)
    }

    /// The version as sqlx numbers a migration.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::migration_set::migration_version::MigrationVersion;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_version_orders_and_reads_as_the_number_it_was_made_from() {
        assert!(MigrationVersion::new(22) < MigrationVersion::new(33));
        assert_eq!(MigrationVersion::new(33).get(), 33);
        assert_eq!(MigrationVersion::new(33).to_string(), "33");
    }
}
