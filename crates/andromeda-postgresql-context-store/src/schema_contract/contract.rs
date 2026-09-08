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
    /// The lowest version among the embedded migrations.
    first_version: SchemaVersion,
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

    /// The lowest version this migration set owns.
    ///
    /// A database can carry more than one migration set in the single
    /// `_sqlx_migrations` table sqlx gives it: the default tenant's database is
    /// also the platform database, so the platform migrations are recorded
    /// beside the tenant ones. The two sets occupy disjoint version ranges —
    /// they must, since one table cannot record two migrations under one
    /// version — so this floor is what tells a row of this set apart from a row
    /// of another one when the applied history is read back.
    #[must_use]
    pub const fn first_version(&self) -> SchemaVersion {
        self.first_version
    }

    /// The stamped schema version.
    #[must_use]
    pub const fn version(&self) -> SchemaVersion {
        self.version
    }

    /// The stamped migration-set digest.
    #[must_use]
    pub const fn digest(&self) -> MigrationsDigest {
        self.digest
    }

    /// The contract a migration set expresses.
    ///
    /// Down migrations carry the version of the up migration they revert, so
    /// they are excluded from both versions as they are from the digest.
    fn over<'migration>(migrations: impl IntoIterator<Item = &'migration Migration> + Clone) -> Result<SchemaContract, SchemaContractError> {
        let versions = migrations
            .clone() // clone: the set is walked twice, once for the versions and once for the digest
            .into_iter()
            .filter(|migration| !migration.migration_type.is_down_migration())
            .map(|migration| migration.version);
        let (lowest, highest) = versions.fold((None, None), |(lowest, highest), version| {
            (
                Some(lowest.map_or(version, |lowest: i64| lowest.min(version))),
                Some(highest.map_or(version, |highest: i64| highest.max(version))),
            )
        });
        let (lowest, highest) = match (lowest, highest) {
            (Some(lowest), Some(highest)) => (lowest, highest),
            (None, _) | (_, None) => return Err(SchemaContractError::NoMigrations),
        };
        Ok(SchemaContract {
            first_version: SchemaVersion::try_from(lowest)?,
            version: SchemaVersion::try_from(highest)?,
            digest: MigrationsDigest::of(migrations),
        })
    }
}

/// Return the tenant schema contract embedded in the running broker binary.
pub fn current_tenant_schema_contract() -> Result<SchemaContract, SchemaContractError> {
    let migrator = sqlx::migrate!("../../migrations/tenant");
    SchemaContract::of(&migrator)
}

#[cfg(test)]
mod tests {
    use crate::schema_contract::{contract::SchemaContract, error::SchemaContractError, migrations_digest::MigrationsDigest};
    use sqlx::migrate::{Migration, MigrationType};
    use std::borrow::Cow;

    /// Build one resolved migration with the shape `migrate!()` produces.
    fn migration(version: i64, description: &'static str, sql: &'static str) -> Migration {
        Migration::new(
            version,
            Cow::Borrowed(description),
            MigrationType::ReversibleUp,
            sqlx::SqlStr::from_static(sql),
            false,
        )
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

    /// The floor is what separates this set's applied rows from those of
    /// another set sharing the database's one `_sqlx_migrations` table.
    #[test]
    fn the_contract_reports_the_lowest_version_the_set_owns() {
        let migrations = vec![
            migration(39, "dictionary growth guard", "ALTER TABLE attribute_names ADD COLUMN guard BOOLEAN"),
            migration(2, "extensions", "CREATE EXTENSION postgis"),
        ];

        let contract = match SchemaContract::over(&migrations) {
            Ok(value) => value,
            Err(error) => unreachable!("the set has a contract: {error}"),
        };
        pretty_assertions::assert_eq!(contract.first_version().get(), 2);
        pretty_assertions::assert_eq!(contract.version().get(), 39);
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
                sqlx::SqlStr::from_static("DROP TABLE subscriptions"),
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
