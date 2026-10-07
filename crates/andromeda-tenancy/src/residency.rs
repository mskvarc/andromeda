//! Whether a tenant's runtime is currently held open in the cache.

/// Whether a tenant has a runtime resident in the cache right now.
///
/// A two-variant enum rather than a bare `bool` so a caller reads intent at the
/// call site: residency is one of the two inputs the computed
/// `TenantState` folds together with the
/// persisted [`TenantStatus`](crate::status::TenantStatus).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TenantResidency {
    /// A runtime for the tenant is held open in the cache.
    Resident,
    /// No runtime for the tenant is held open.
    NotResident,
}
