//! The platform-database relations that track tenancy.
//!
//! The `tenants` registry maps a tenant name to the database that holds its
//! context, and records the unnamed default tenant as a row like any other. It
//! lives in the platform database, not a tenant database, so it is the one
//! relation that can answer which tenants exist after every tenant runtime has
//! been evicted. `schema_metadata` is the per-database singleton that records
//! which schema contract a database was stamped with.

use sea_query::Iden;

/// The longest database name PostgreSQL keeps, in bytes (`NAMEDATALEN - 1` in a
/// default build). `tenants.database_name` is held to it by a check, and every
/// name the broker generates fits within it.
pub const DATABASE_NAME_BYTE_LIMIT: usize = 63;

/// The `tenants` table - the platform registry of tenant to database mappings.
#[derive(Iden, Clone, Copy)]
pub enum Tenants {
    /// Table name: `tenants`
    Table,
    /// Surrogate primary key
    TenantKey,
    /// Tenant name, `NULL` for the unnamed default tenant
    Name,
    /// Whether this row is the unnamed default tenant
    IsUnnamedDefault,
    /// Name of the database holding the tenant's context
    DatabaseName,
    /// Tenant administrative status (`TenantStatusCode`)
    Status,
    /// Instant the row was created
    CreatedAt,
    /// Instant the row was last modified
    ModifiedAt,
}

/// The `tenant_wake_schedule` table - when each tenant should next be loaded to
/// run background work while no request is touching it.
#[derive(Iden, Clone, Copy)]
pub enum TenantWakeSchedule {
    /// Table name: `tenant_wake_schedule`
    Table,
    /// Foreign key to the owning `tenants` row
    TenantKey,
    /// Instant the tenant next has work due, `NULL` when nothing is pending
    WakeAt,
    /// Instant the schedule was last written
    ProbedAt,
}

/// The `schema_metadata` table - the per-database schema-contract stamp.
#[derive(Iden, Clone, Copy)]
pub enum SchemaMetadata {
    /// Table name: `schema_metadata`
    Table,
    /// Instant the stamp was last written
    AppliedAt,
    /// Schema version the database is stamped with
    SchemaVersion,
    /// Singleton discriminator, always `TRUE`, enforcing one row
    Singleton,
    /// Digest of the migrations the stamp attests to
    MigrationsSha256,
}

#[cfg(test)]
mod tests {
    use crate::tenancy::{SchemaMetadata, TenantWakeSchedule, Tenants};
    use pretty_assertions::assert_eq;
    use sea_query::Iden;

    #[test]
    fn schema_metadata_table_and_columns() {
        assert_eq!(SchemaMetadata::Table.to_string(), "schema_metadata");
        assert_eq!(SchemaMetadata::AppliedAt.to_string(), "applied_at");
        assert_eq!(SchemaMetadata::SchemaVersion.to_string(), "schema_version");
        assert_eq!(SchemaMetadata::Singleton.to_string(), "singleton");
        assert_eq!(SchemaMetadata::MigrationsSha256.to_string(), "migrations_sha256");
    }

    #[test]
    fn tenants_table_name() {
        assert_eq!(Tenants::Table.to_string(), "tenants");
    }

    #[test]
    fn tenant_wake_schedule_table_and_columns() {
        assert_eq!(TenantWakeSchedule::Table.to_string(), "tenant_wake_schedule");
        assert_eq!(TenantWakeSchedule::TenantKey.to_string(), "tenant_key");
        assert_eq!(TenantWakeSchedule::WakeAt.to_string(), "wake_at");
        assert_eq!(TenantWakeSchedule::ProbedAt.to_string(), "probed_at");
    }

    #[test]
    fn tenants_column_names() {
        assert_eq!(Tenants::TenantKey.to_string(), "tenant_key");
        assert_eq!(Tenants::Name.to_string(), "name");
        assert_eq!(Tenants::IsUnnamedDefault.to_string(), "is_unnamed_default");
        assert_eq!(Tenants::DatabaseName.to_string(), "database_name");
        assert_eq!(Tenants::Status.to_string(), "status");
        assert_eq!(Tenants::CreatedAt.to_string(), "created_at");
        assert_eq!(Tenants::ModifiedAt.to_string(), "modified_at");
    }
}
