//! One migration file: the set it is kept in and its name.

use crate::{
    migration_name::{MigrationName, MigrationNameError},
    migration_set::MigrationSet,
    migration_version::MigrationVersion,
};
use std::path::{Path, PathBuf};

/// A migration file whose name follows the naming rule.
#[derive(Debug, Clone, PartialEq, Eq, derive_more::Display)]
#[display("migrations/{set}/{name}")]
pub struct MigrationFile {
    /// The directory the file is kept in.
    set: MigrationSet,
    /// Its file name.
    name: MigrationName,
    /// Where the file is on disk.
    path: PathBuf,
}

impl MigrationFile {
    /// The migration at `path`, kept in the directory of `set`.
    ///
    /// # Errors
    ///
    /// Returns [`MigrationFileError`] when the file name does not follow the
    /// naming rule.
    pub fn new(set: MigrationSet, path: PathBuf) -> Result<MigrationFile, MigrationFileError> {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(MigrationNameError::NotUtf8)
            .and_then(str::parse);
        match name {
            Ok(name) => Ok(MigrationFile { set, name, path }),
            Err(reason) => Err(MigrationFileError { path, reason }),
        }
    }

    /// The directory the file is kept in.
    #[must_use]
    pub const fn set(&self) -> MigrationSet {
        self.set
    }

    /// Its file name.
    #[must_use]
    pub const fn name(&self) -> &MigrationName {
        &self.name
    }

    /// The version the file carries.
    #[must_use]
    pub const fn version(&self) -> MigrationVersion {
        self.name.version()
    }

    /// The file's path relative to the repository root, `migrations/<set>/<name>`.
    #[must_use]
    pub fn repository_path(&self) -> PathBuf {
        PathBuf::from(self.to_string())
    }

    /// Where the file is on disk.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// A file below a migration directory whose name breaks the naming rule.
#[derive(Debug, thiserror::Error)]
#[error("{} does not match NNNN_snake_case.sql: {reason}", path.display())]
pub struct MigrationFileError {
    /// The file concerned.
    path: PathBuf,
    /// Which part of the rule the name breaks.
    reason: MigrationNameError,
}

#[cfg(test)]
mod tests {
    use crate::{migration_file::MigrationFile, migration_name::MigrationNameError, migration_set::MigrationSet, migration_version::MigrationVersionError};
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    #[test]
    fn a_conforming_file_is_read_into_its_set_version_and_name() {
        let file = MigrationFile::new(
            MigrationSet::Tenant,
            PathBuf::from("/root/migrations/tenant/0029_subscription_notified_registrations.sql"),
        )
        .ok();
        assert_eq!(
            file.map(|file| (file.version().get(), file.to_string())),
            Some((29, "migrations/tenant/0029_subscription_notified_registrations.sql".to_owned()))
        );
    }

    #[test]
    fn a_misnamed_file_keeps_its_path_and_the_part_it_breaks() {
        let error = MigrationFile::new(MigrationSet::Tenant, PathBuf::from("/root/migrations/tenant/032_types.sql")).err();
        assert_eq!(
            error.map(|error| (error.path, error.reason)),
            Some((
                PathBuf::from("/root/migrations/tenant/032_types.sql"),
                MigrationNameError::Version(MigrationVersionError::NotFourDigits)
            ))
        );
    }
}
