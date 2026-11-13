//! The tenant schema as the v1.0.0 release sealed it.
//!
//! A sealed migration never changes, so the embedded migrations up to the
//! last one v1.0.0 sealed build exactly the schema a v1.0.0 tenant holds, in
//! `public` and in every Snapshot branch schema alike. A rehearsal brings a
//! schema there with the broker's own migrators, recorded in the ledgers the
//! broker reads, so the upgrade resumes where a deployed v1.0.0 tenant
//! resumes.

use andromeda_postgresql_context_store::migration_set::migration_version::MigrationVersion;

/// The highest migration version v1.0.0 sealed.
pub const V1_0_0_LAST_MIGRATION: MigrationVersion = MigrationVersion::new(30);

/// The last branch migration v1.0.0 sealed, the notification queue: every
/// Snapshot branch schema of a v1.0.0 tenant holds the branch migrations
/// through it.
pub const V1_0_0_LAST_BRANCH_MIGRATION: MigrationVersion = MigrationVersion::new(22);

#[cfg(test)]
mod tests {
    use crate::release_schema::{V1_0_0_LAST_BRANCH_MIGRATION, V1_0_0_LAST_MIGRATION};
    use andromeda_postgresql_context_store::migration_set::{branch::branch_migrations, migration_version::MigrationVersion};
    use pretty_assertions::assert_eq;

    #[test]
    fn the_last_branch_migration_v1_0_0_sealed_is_the_last_embedded_one_at_or_below_its_last_migration() {
        let sealed = branch_migrations()
            .iter()
            .map(|migration| MigrationVersion::new(migration.version))
            .filter(|version| *version <= V1_0_0_LAST_MIGRATION)
            .max();
        assert_eq!(sealed, Some(V1_0_0_LAST_BRANCH_MIGRATION));
    }
}
