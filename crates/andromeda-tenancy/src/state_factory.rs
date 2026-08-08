//! Building one tenant's runtime, which this crate asks for and cannot do.

use crate::{database_name::TenantDatabaseName, error::TenancyError, tenant_ref::TenantRef};
use async_trait::async_trait;
use std::sync::Arc;

/// Everything one tenant runtime holds open.
///
/// Opaque to this crate: a runtime is a connection pool, a set of workers and
/// a set of transports, none of which a tenant cache has any business naming.
/// All it needs is the ability to let go of them.
#[async_trait]
pub trait TenantRuntime: Send + Sync + 'static {
    /// Release everything the runtime holds: stop the workers, close the
    /// transports, return the connections.
    ///
    /// Takes `self` because a released runtime must not be reachable
    /// afterwards, and returning it to the cache is the bug this signature
    /// makes unwritable.
    #[must_use = "runtime shutdown must be awaited"]
    async fn shutdown(self);
}

/// Building everything a tenant needs in order to serve requests.
///
/// Implemented by the composition root, because building a tenant's runtime
/// means naming every service the broker has — which is exactly what this
/// crate must not do. The inversion is the point: the cache decides *when* a
/// runtime is needed and *when* it is dropped, and knows nothing about what is
/// in one.
#[async_trait]
pub trait TenantStateFactory: Send + Sync + 'static {
    /// The per-tenant state a request handler reads.
    type State: Send + Sync + 'static;
    /// The resources built alongside that state.
    type Runtime: TenantRuntime;

    /// Build a runtime for one tenant against the database it was provisioned
    /// in.
    #[must_use = "tenant runtime construction must be awaited"]
    async fn build(&self, tenant: &TenantRef, database: &TenantDatabaseName) -> Result<(Arc<Self::State>, Self::Runtime), TenancyError>;
}
