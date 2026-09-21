//! One tenant's registry row, as the registry hands it back.

use crate::{database_name::TenantDatabaseName, status::TenantStatus};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use chrono::{DateTime, Utc};

/// A named tenant's registry row: its name, administrative state, database and
/// system timestamps.
///
/// The registry projects this for both the per-tenant introspection resource and
/// the tenant collection, and the runtime cache reads it on a cold admission to
/// learn both where a tenant's data lives and whether it may be served at all.
/// Every field is a domain type; nothing here is a bare string.
#[derive(Debug, Clone)]
pub struct TenantRecord {
    /// The tenant's validated name.
    pub tenant_name: TenantName,
    /// The persisted administrative state (the write axis of lifecycle control).
    pub status: TenantStatus,
    /// The database holding the tenant's context.
    pub database: TenantDatabaseName,
    /// When the registry row was created (clause 4.8 `createdAt`).
    pub created_at: DateTime<Utc>,
    /// When the registry row was last modified (clause 4.8 `modifiedAt`).
    pub modified_at: DateTime<Utc>,
}

impl TenantRecord {
    /// Assemble a tenant record from its registry columns.
    #[must_use]
    pub const fn new(
        tenant_name: TenantName,
        status: TenantStatus,
        database: TenantDatabaseName,
        created_at: DateTime<Utc>,
        modified_at: DateTime<Utc>,
    ) -> TenantRecord {
        TenantRecord {
            tenant_name,
            status,
            database,
            created_at,
            modified_at,
        }
    }
}
