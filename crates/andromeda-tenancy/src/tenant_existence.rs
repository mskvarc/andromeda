//! Whether a tenant exists yet, as the registry and the runtime cache see it.
//!
//! A tenant is provisional from the moment a creating request registers it
//! until its database is seen to hold information (clause 5.5.10). The runtime
//! cache decides which requests a provisional tenant serves from this, and a
//! pending-work probe decides how it may reach the tenant's database: a
//! provisional tenant's database may not exist yet, or may not carry the
//! session defaults its provisioning installs.

/// Whether the tenant exists yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TenantExistence {
    /// Provisional: a request that requires the tenant to exist must first see
    /// its database hold information (clause 5.5.10), and its database may
    /// still be being created, configured or migrated.
    Provisional,
    /// Established: the tenant exists, every request is served, and its
    /// database is created and carries its session defaults.
    Established,
}
