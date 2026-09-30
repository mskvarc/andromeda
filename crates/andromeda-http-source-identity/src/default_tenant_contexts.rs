//! The contexts `/info/sourceIdentity` is answered under.
//!
//! The identity is answered before any tenant is resolved (clause 5.15.1.4
//! gives the operation no failure that depends on a tenant existing), but the
//! `@context` its response names still has to be resolved and, for an
//! `application/json` response naming several contexts, hosted. Clause 4.14:
//! "any operation not explicitly specifying a Tenant targets a default Tenant,
//! which always exists", so the response is resolved with the default tenant's
//! loader and hosted among the default tenant's stored `@context`s.

use andromeda_http_context_registry::tenant_context_hosting::TenantContextHosting;
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_tenancy::{error::TenancyError, runtime::lease::RuntimeLease};
use futures::future::BoxFuture;

/// The default tenant's context resolution and hosting.
pub struct DefaultTenantContexts {
    /// The default tenant's JSON-LD processor.
    pub engine: JsonLdEngine,
    /// The default tenant's host of the contexts a response names.
    pub context_hosting: TenantContextHosting,
    /// Keeps the default tenant's runtime serving for the life of the request,
    /// so a wrapper hosted during shutdown never reaches a pool being torn
    /// down.
    pub runtime_lease: RuntimeLease,
}

/// Resolves the default tenant's context resolution and hosting.
pub trait DefaultTenantContextsResolver: Send + Sync {
    /// The default tenant's contexts.
    fn resolve(&self) -> BoxFuture<'_, Result<DefaultTenantContexts, TenancyError>>;
}
