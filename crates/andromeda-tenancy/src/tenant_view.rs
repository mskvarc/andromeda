//! A tenant's registry row paired with its current runtime residency.

use crate::{residency::TenantResidency, tenant_record::TenantRecord};

/// What the broker knows about one tenant without touching its database: the
/// persisted registry row and whether a runtime for it is currently resident.
///
/// This is the cheap-by-default half of tenant introspection. The wire status a
/// tenant resource reports is derived from these two by the HTTP layer, and the
/// expensive operational usage ([`TenantUsage`](crate::tenant_usage::TenantUsage))
/// is read separately and only when asked for, so a plain introspection read never
/// queries a tenant database and never disturbs dormancy.
#[derive(Debug, Clone)]
pub struct TenantView {
    /// The tenant's registry row.
    pub record: TenantRecord,
    /// Whether a runtime for the tenant is currently held open.
    pub residency: TenantResidency,
}

impl TenantView {
    /// Pair a registry row with its current runtime residency.
    #[must_use]
    pub const fn new(record: TenantRecord, residency: TenantResidency) -> TenantView {
        TenantView { record, residency }
    }
}
