//! Building one tenant's runtime, which this crate asks for and cannot do.

use crate::{error::TenancyError, tenant_ref::TenantRef};
use andromeda_ngsi_ld_lexicon::tenant_database_name::TenantDatabaseName;
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

    /// Where the cache reports use of this runtime.
    ///
    /// The cache keeps the handle beside the runtime's eviction origin and
    /// reports on it whenever a request is handed the runtime and whenever a
    /// request lets it go, so a runtime pacing its background work by what is
    /// due learns of every request that may have changed that.
    fn activity(&self) -> Arc<dyn RuntimeActivity>;
}

/// Told when requests reach for, and let go of, one tenant runtime.
///
/// A request may write work that falls due later (an `expiresAt`, a
/// periodic subscription, a queued delivery) and it has committed that work
/// by the time it lets the runtime go, so a runtime that caches what it last
/// read as due must read again after either report.
pub trait RuntimeActivity: Send + Sync + 'static {
    /// Note that a request reached for or let go of the runtime.
    fn record_activity(&self);
}

/// Building everything a tenant needs in order to serve requests.
///
/// Implemented by the composition root, because building a tenant's runtime
/// means naming every service the broker has, which is exactly what this
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
