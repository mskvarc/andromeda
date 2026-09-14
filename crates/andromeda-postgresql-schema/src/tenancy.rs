//! The platform-database relations that track tenancy.
//!
//! The `tenants` registry maps a tenant name to the database that holds its
//! context, and records the unnamed default tenant as a row like any other. It
//! lives in the platform database, not a tenant database, so it is the one
//! relation that can answer which tenants exist after every tenant runtime has
//! been evicted. `schema_metadata` is the per-database singleton that records
//! which schema contract a database was stamped with.

use sea_query::Iden;

/// The `tenants` table - the platform registry of tenant to database mappings.
#[derive(Iden, Clone, Copy)]
pub enum Tenants {
    /// Table name: `tenants`
    Table,
    /// Surrogate primary key
    Id,
    /// Tenant name, `NULL` for the unnamed default tenant
    Name,
    /// Whether this row is the unnamed default tenant
    IsUnnamedDefault,
    /// Name of the database holding the tenant's context
    DbName,
    /// Tenant lifecycle status
    Status,
    /// Instant the row was created
    CreatedAt,
    /// Instant the row was last modified
    ModifiedAt,
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
    use crate::tenancy::{SchemaMetadata, Tenants};
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
    fn tenants_column_names() {
        assert_eq!(Tenants::Id.to_string(), "id");
        assert_eq!(Tenants::Name.to_string(), "name");
        assert_eq!(Tenants::IsUnnamedDefault.to_string(), "is_unnamed_default");
        assert_eq!(Tenants::DbName.to_string(), "db_name");
        assert_eq!(Tenants::Status.to_string(), "status");
        assert_eq!(Tenants::CreatedAt.to_string(), "created_at");
        assert_eq!(Tenants::ModifiedAt.to_string(), "modified_at");
    }
}
