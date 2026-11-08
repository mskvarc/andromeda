//! One section of the seal ledger: the migrations one release added, each with
//! the digest of its committed bytes.

use crate::{
    file_digest::FileDigest,
    migration_file::MigrationFile,
    migration_name::MigrationName,
    migration_set::MigrationSet,
    migration_version::MigrationVersion,
    release_version::ReleaseVersion,
};

/// One migration a release sealed.
#[derive(Debug, Clone, PartialEq, Eq, derive_more::Display, serde::Serialize, serde::Deserialize)]
#[display("migrations/{set}/{name}")]
#[serde(deny_unknown_fields)]
pub struct SealedMigration {
    /// The directory the file is kept in.
    set: MigrationSet,
    /// Its file name.
    name: MigrationName,
    /// The SHA-256 of its raw bytes when sealed.
    sha256: FileDigest,
}

impl SealedMigration {
    /// `file` sealed with the digest `sha256` of its bytes.
    #[must_use]
    pub fn new(file: &MigrationFile, sha256: FileDigest) -> SealedMigration {
        SealedMigration {
            set: file.set(),
            // clone: the ledger keeps the name beyond the file it was read from
            name: file.name().clone(),
            sha256,
        }
    }

    /// Whether this is the seal of `file`: the same set and file name.
    #[must_use]
    pub fn is_of(&self, file: &MigrationFile) -> bool {
        self.set == file.set() && &self.name == file.name()
    }

    /// Whether `other` seals the same file: the same set and file name.
    #[must_use]
    pub fn is_same_file(&self, other: &SealedMigration) -> bool {
        self.set == other.set && self.name == other.name
    }

    /// The version the sealed file carries.
    #[must_use]
    pub const fn version(&self) -> MigrationVersion {
        self.name.version()
    }

    /// The SHA-256 of its raw bytes when sealed.
    #[must_use]
    pub const fn sha256(&self) -> FileDigest {
        self.sha256
    }
}

/// The migrations one stable release added.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SealedRelease {
    /// The release.
    version: ReleaseVersion,
    /// The migrations it added, in version order.
    #[serde(rename = "migration", default)]
    migrations: Vec<SealedMigration>,
}

impl SealedRelease {
    /// The section of `version` sealing `migrations`.
    #[must_use]
    pub fn new(version: ReleaseVersion, migrations: Vec<SealedMigration>) -> SealedRelease {
        let mut migrations = migrations;
        migrations.sort_by_key(SealedMigration::version);
        SealedRelease { version, migrations }
    }

    /// The release.
    #[must_use]
    pub const fn version(&self) -> &ReleaseVersion {
        &self.version
    }

    /// The migrations it added, in the order written.
    #[must_use]
    pub fn migrations(&self) -> &[SealedMigration] {
        &self.migrations
    }

    /// The section in canonical order: its migrations in version order.
    #[must_use]
    pub fn canonical(&self) -> SealedRelease {
        SealedRelease::new(self.version.clone(), self.migrations.clone())
    }
}
