//! The pair a tenant database is stamped with.

use crate::schema_contract::{error::SchemaContractError, migrations_digest::MigrationsDigest, schema_version::SchemaVersion};
use sqlx::migrate::{Migration, Migrator};

/// What the running binary requires of a tenant's schema.
///
/// Both halves are derived from the embedded migrator and never from the
/// filesystem at runtime: the version answers "how far", the digest answers
/// "along which path", and only the two together distinguish a tenant that is
/// behind from a fleet running two different binaries at the same version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchemaContract {
    /// The highest version among the embedded migrations.
    version: SchemaVersion,
    /// The digest over the whole embedded migration set.
    digest: MigrationsDigest,
}

impl SchemaContract {
    /// The contract an embedded migrator expresses.
    pub fn of(migrator: &Migrator) -> Result<SchemaContract, SchemaContractError> {
        SchemaContract::over(migrator.iter())
    }

    /// The stamped schema version.
    pub const fn version(&self) -> SchemaVersion {
        self.version
    }

    /// The stamped migration-set digest.
    pub const fn digest(&self) -> MigrationsDigest {
        self.digest
    }

    /// The contract a migration set expresses.
    ///
    /// Down migrations carry the version of the up migration they revert, so
    /// they are excluded from the version as they are from the digest.
    fn over<'migration>(migrations: impl IntoIterator<Item = &'migration Migration> + Clone) -> Result<SchemaContract, SchemaContractError> {
        let highest = migrations
            .clone() // clone: the set is walked twice, once for the version and once for the digest
            .into_iter()
            .filter(|migration| !migration.migration_type.is_down_migration())
            .map(|migration| migration.version)
            .max()
            .ok_or(SchemaContractError::NoMigrations)?;
        Ok(SchemaContract {
            version: SchemaVersion::try_from(highest)?,
            digest: MigrationsDigest::of(migrations),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::schema_contract::{contract::SchemaContract, error::SchemaContractError, migrations_digest::MigrationsDigest};
    use sqlx::migrate::{Migration, MigrationType};
    use std::borrow::Cow;

    /// Build one resolved migration with the shape `migrate!()` produces.
    fn migration(version: i64, description: &'static str, sql: &'static str) -> Migration {
        Migration::new(version, Cow::Borrowed(description), MigrationType::ReversibleUp, Cow::Borrowed(sql), false)
    }

    #[test]
    fn the_contract_is_the_highest_migration_version_and_the_digest_of_the_whole_set() {
        let migrations = vec![
            migration(2, "extensions", "CREATE EXTENSION postgis"),
            migration(
                39,
                "dictionary growth guard",
                "ALTER TABLE attribute_names ALTER COLUMN name TYPE TEXT COLLATE \"C\"",
            ),
        ];

        let contract = match SchemaContract::over(&migrations) {
            Ok(value) => value,
            Err(error) => unreachable!("the set has a contract: {error}"),
        };
        pretty_assertions::assert_eq!(contract.version().get(), 39);
        pretty_assertions::assert_eq!(contract.digest(), MigrationsDigest::of(&migrations));
    }

    #[test]
    fn an_empty_migration_set_stamps_nothing_rather_than_stamping_version_zero() {
        let migrations: Vec<Migration> = Vec::new();
        assert!(matches!(SchemaContract::over(&migrations), Err(SchemaContractError::NoMigrations)));
    }

    #[test]
    fn a_down_migration_does_not_raise_the_stamped_version() {
        let migrations = vec![
            migration(7, "temporal entity attributes", "CREATE TABLE temporal_entity_attributes ()"),
            Migration::new(
                9,
                Cow::Borrowed("subscriptions"),
                MigrationType::ReversibleDown,
                Cow::Borrowed("DROP TABLE subscriptions"),
                false,
            ),
        ];

        let contract = match SchemaContract::over(&migrations) {
            Ok(value) => value,
            Err(error) => unreachable!("the set has a contract: {error}"),
        };
        pretty_assertions::assert_eq!(contract.version().get(), 7);
    }
}
