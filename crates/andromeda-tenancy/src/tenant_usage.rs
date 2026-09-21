//! The expensive, opt-in operational usage a tenant introspection read can carry.

/// Per-tenant operational usage, gathered by querying the tenant's own database.
///
/// This is the `?options=usage` half of a tenant resource: it costs a tenant-DB
/// round trip, so it is never gathered for a plain introspection read. Every
/// member is a broker-defined operational metric, not an NGSI-LD term.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TenantUsage {
    /// Live PostgreSQL backend connections serving the tenant's database.
    pub pool_connections: u64,
    /// Total on-disk size of the tenant's database, in bytes.
    pub database_size: u64,
    /// Number of current entities the tenant holds.
    pub entity_count: u64,
    /// Number of snapshots the tenant holds.
    pub snapshot_count: u64,
    /// The configured ceiling on the tenant's snapshot count, if one is set.
    pub snapshot_quota: Option<u64>,
}
