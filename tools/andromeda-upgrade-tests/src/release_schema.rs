//! The tenant schema as the v1.0.0 release sealed it.
//!
//! A sealed migration never changes, so the embedded migrations up to the
//! last one v1.0.0 sealed build exactly the schema a v1.0.0 tenant holds. The
//! migrator here is the broker's own tenant set cut at that version and
//! recorded in the tenant ledger, so running the full set afterwards resumes
//! where a deployed v1.0.0 tenant resumes.

use andromeda_postgresql_context_store::migration_set::tenant::{TENANT_LEDGER, tenant_migrator};
use sqlx::migrate::Migrator;
use std::borrow::Cow;

/// The highest migration version v1.0.0 sealed.
pub const V1_0_0_LAST_MIGRATION: i64 = 30;

/// The tenant migrations a v1.0.0 tenant has applied, recorded in the tenant
/// ledger.
#[must_use]
pub fn v1_0_0_migrator() -> Migrator {
    // clone: the embedded set is a static slice, and the cut set owns its subset
    let migrations = tenant_migrator()
        .iter()
        .filter(|migration| migration.version <= V1_0_0_LAST_MIGRATION)
        .cloned()
        .collect::<Vec<_>>();
    let mut migrator = Migrator {
        migrations: Cow::Owned(migrations),
        ..Migrator::DEFAULT
    };
    migrator.dangerous_set_table_name(TENANT_LEDGER);
    migrator
}

#[cfg(test)]
mod tests {
    use crate::release_schema::{V1_0_0_LAST_MIGRATION, v1_0_0_migrator};
    use pretty_assertions::assert_eq;

    /// The last sealed version belongs to the platform set, so the tenant and
    /// branch files the migrator holds end just below it.
    #[test]
    fn the_v1_0_0_migrator_holds_every_tenant_migration_below_the_last_sealed_version() {
        let versions: Vec<i64> = v1_0_0_migrator().iter().map(|migration| migration.version).collect();
        let tenant_and_branch_from_five: Vec<i64> = (5..V1_0_0_LAST_MIGRATION).collect();
        assert_eq!(versions, tenant_and_branch_from_five);
    }
}
