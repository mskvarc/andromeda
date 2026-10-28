//! The seal ledger, `migrations/seal-ledger.toml`: for every stable release,
//! the migrations it added and the SHA-256 of each one's raw committed bytes.
//!
//! A section holds only the files its release added, so a sealed section never
//! changes when a later release is sealed. The tool writes the file in one
//! canonical form (sections in release order, migrations in version order),
//! so a hand edit shows as a difference from that form.

/// One section of the ledger.
pub mod sealed_release;

use crate::{
    migration_file::MigrationFile,
    migration_version::MigrationVersion,
    seal_ledger::sealed_release::{SealedMigration, SealedRelease},
};
use std::{
    fs,
    io,
    path::{Path, PathBuf},
};
use toml::{de::Error as DeserializeError, ser::Error as SerializeError};

/// The ledger's file name, inside `migrations/`.
pub const LEDGER_FILE_NAME: &str = "seal-ledger.toml";

/// The ledger's path relative to the repository root.
pub const LEDGER_PATH: &str = "migrations/seal-ledger.toml";

/// The comment the canonical form opens with.
const LEDGER_HEADER: &str = "# Written by `andromeda-sql seal`. A sealed migration and its section never change.\n\n";

/// The hash every digest of the ledger is computed with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum HashAlgorithm {
    /// SHA-256 over the raw bytes of the file.
    #[default]
    #[serde(rename = "sha-256")]
    Sha256,
}

/// Every release sealed so far.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SealLedger {
    /// The hash the digests are computed with.
    algorithm: HashAlgorithm,
    /// The sections, in the order written.
    #[serde(rename = "release", default, skip_serializing_if = "Vec::is_empty")]
    releases: Vec<SealedRelease>,
}

impl SealLedger {
    /// Where the ledger of the repository at `root` is.
    #[must_use]
    pub fn path(root: &Path) -> PathBuf {
        root.join(LEDGER_PATH)
    }

    /// The text of the ledger at `root`, absent when no release is sealed yet.
    ///
    /// # Errors
    ///
    /// Returns [`SealLedgerError::Read`] when the file exists but cannot be
    /// read.
    pub fn read_text(root: &Path) -> Result<Option<String>, SealLedgerError> {
        let path = SealLedger::path(root);
        match fs::read_to_string(&path) {
            Ok(text) => Ok(Some(text)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(SealLedgerError::Read { path, source }),
        }
    }

    /// The ledger at `root`, empty when no release is sealed yet.
    ///
    /// # Errors
    ///
    /// Returns [`SealLedgerError`] when the file cannot be read or is not a
    /// ledger.
    pub fn load(root: &Path) -> Result<SealLedger, SealLedgerError> {
        SealLedger::read_text(root)?.map_or_else(|| Ok(SealLedger::default()), |text| SealLedger::parse(&text))
    }

    /// The ledger `text` holds.
    ///
    /// # Errors
    ///
    /// Returns [`SealLedgerError::Unparsed`] when `text` is not a ledger.
    pub fn parse(text: &str) -> Result<SealLedger, SealLedgerError> {
        toml::from_str(text).map_err(SealLedgerError::Unparsed)
    }

    /// The ledger in its canonical form, as the tool writes it.
    ///
    /// # Errors
    ///
    /// Returns [`SealLedgerError::Unrendered`] when the ledger cannot be
    /// written as TOML.
    pub fn render(&self) -> Result<String, SealLedgerError> {
        let mut releases = self.releases.iter().map(SealedRelease::canonical).collect::<Vec<_>>();
        releases.sort_by(|left, right| left.version().cmp(right.version()));
        let canonical = SealLedger {
            algorithm: self.algorithm,
            releases,
        };
        let body = toml::to_string(&canonical).map_err(SealLedgerError::Unrendered)?;
        Ok(format!("{LEDGER_HEADER}{body}"))
    }

    /// Write the ledger in its canonical form to `root`.
    ///
    /// # Errors
    ///
    /// Returns [`SealLedgerError`] when it cannot be rendered or written.
    pub fn write(&self, root: &Path) -> Result<(), SealLedgerError> {
        let path = SealLedger::path(root);
        let text = self.render()?;
        fs::write(&path, text).map_err(|source| SealLedgerError::Write { path, source })
    }

    /// The ledger with `release` appended.
    #[must_use]
    pub fn with_release(self, release: SealedRelease) -> SealLedger {
        let mut releases = self.releases;
        releases.push(release);
        SealLedger {
            algorithm: self.algorithm,
            releases,
        }
    }

    /// The sections, in the order written.
    #[must_use]
    pub fn releases(&self) -> &[SealedRelease] {
        &self.releases
    }

    /// The section of the latest release.
    #[must_use]
    pub fn latest_release(&self) -> Option<&SealedRelease> {
        self.releases.iter().max_by(|left, right| left.version().cmp(right.version()))
    }

    /// Every sealed migration, with the release that sealed it.
    pub fn sealed_migrations(&self) -> impl Iterator<Item = (&SealedRelease, &SealedMigration)> {
        self.releases
            .iter()
            .flat_map(|release| release.migrations().iter().map(move |migration| (release, migration)))
    }

    /// Whether a release sealed `file`.
    #[must_use]
    pub fn seals(&self, file: &MigrationFile) -> bool {
        self.sealed_migrations().any(|(_, migration)| migration.is_of(file))
    }

    /// Whether a release sealed the migration holding `version`.
    #[must_use]
    pub fn seals_version(&self, version: MigrationVersion) -> bool {
        self.sealed_migrations().any(|(_, migration)| migration.version() == version)
    }

    /// The highest version a release sealed.
    #[must_use]
    pub fn highest_sealed_version(&self) -> Option<MigrationVersion> {
        self.sealed_migrations().map(|(_, migration)| migration.version()).max()
    }
}

/// Why the ledger could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum SealLedgerError {
    /// The ledger exists but cannot be read.
    #[error("{} could not be read: {source}", path.display())]
    Read {
        /// The ledger's path.
        path: PathBuf,
        /// What reading it failed with.
        source: io::Error,
    },
    /// The ledger is not a seal ledger.
    #[error("{LEDGER_PATH} is not a seal ledger: {0}")]
    Unparsed(DeserializeError),
    /// The ledger cannot be written as TOML.
    #[error("{LEDGER_PATH} could not be rendered: {0}")]
    Unrendered(SerializeError),
    /// The ledger could not be written.
    #[error("{} could not be written: {source}", path.display())]
    Write {
        /// The ledger's path.
        path: PathBuf,
        /// What writing it failed with.
        source: io::Error,
    },
}

#[cfg(test)]
mod tests {
    use crate::{
        file_digest::{FileDigest, FileDigestError},
        migration_file::{MigrationFile, MigrationFileError},
        migration_set::MigrationSet,
        release_version::ReleaseVersionError,
        seal_ledger::{
            SealLedger,
            SealLedgerError,
            sealed_release::{SealedMigration, SealedRelease},
        },
    };
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    /// The digest every fixture file is sealed with.
    const DIGEST: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    /// Why a fixture ledger could not be built.
    #[derive(Debug, thiserror::Error)]
    enum FixtureError {
        /// A fixture file name breaks the naming rule.
        #[error(transparent)]
        File(#[from] MigrationFileError),
        /// The fixture digest does not parse.
        #[error(transparent)]
        Digest(#[from] FileDigestError),
        /// A fixture release does not parse.
        #[error(transparent)]
        Version(#[from] ReleaseVersionError),
        /// The ledger could not be rendered or read back.
        #[error(transparent)]
        Ledger(#[from] SealLedgerError),
    }

    fn sealed(set: MigrationSet, name: &str) -> Result<SealedMigration, FixtureError> {
        let file = MigrationFile::new(set, PathBuf::from("/root/migrations").join(set.to_string()).join(name))?;
        Ok(SealedMigration::new(&file, DIGEST.parse::<FileDigest>()?))
    }

    #[test]
    fn the_canonical_form_orders_releases_and_migrations_and_reads_back() -> Result<(), FixtureError> {
        let ledger = SealLedger::default()
            .with_release(SealedRelease::new("v1.1.0".parse()?, vec![sealed(MigrationSet::Tenant, "0003_maps.sql")?]))
            .with_release(SealedRelease::new(
                "v1.0.0".parse()?,
                vec![
                    sealed(MigrationSet::Tenant, "0002_types.sql")?,
                    sealed(MigrationSet::Platform, "0001_tenants.sql")?,
                ],
            ));
        let rendered = ledger.render()?;
        assert_eq!(
            rendered,
            format!(
                "# Written by `andromeda-sql seal`. A sealed migration and its section never change.\n\nalgorithm = \"sha-256\"\n\n[[release]]\nversion = \"v1.0.0\"\n\n[[release.migration]]\nset = \"platform\"\nname = \"0001_tenants.sql\"\nsha256 = \"{DIGEST}\"\n\n[[release.migration]]\nset = \"tenant\"\nname = \"0002_types.sql\"\nsha256 = \"{DIGEST}\"\n\n[[release]]\nversion = \"v1.1.0\"\n\n[[release.migration]]\nset = \"tenant\"\nname = \"0003_maps.sql\"\nsha256 = \"{DIGEST}\"\n"
            )
        );
        assert_eq!(SealLedger::parse(&rendered)?.render()?, rendered);
        Ok(())
    }

    #[test]
    fn a_ledger_naming_a_pre_release_or_an_unknown_key_is_not_a_ledger() {
        let pre_release = "algorithm = \"sha-256\"\n\n[[release]]\nversion = \"v1.0.0-rc.1\"\n";
        let error = SealLedger::parse(pre_release).err();
        assert!(matches!(error, Some(SealLedgerError::Unparsed(_))), "{error:?}");
        let message = error.map(|error| error.to_string()).unwrap_or_default();
        assert!(
            message.contains("the tag names the pre-release rc.1, and only a stable release vX.Y.Z is sealed"),
            "{message}"
        );
        assert!(SealLedger::parse("algorithm = \"sha-256\"\nsealed_by = \"someone\"\n").is_err());
        assert!(SealLedger::parse("algorithm = \"md5\"\n").is_err());
    }
}
