//! What happens when a request names a tenant the broker has never seen.

/// Whether an unknown tenant is created on first access or rejected.
///
/// Read per endpoint rather than per deployment: clause 6.3.14 does not say
/// that addressing a tenant creates it, so most routes answer `404` for an
/// unknown one, and only the routes that legitimately bring a tenant into
/// existence pass [`TenantCreationPolicy::CreateOnDemand`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TenantCreationPolicy {
    /// Reject a tenant that is not already registered.
    MustExist,
    /// Register and provision the tenant on first access.
    CreateOnDemand,
}
