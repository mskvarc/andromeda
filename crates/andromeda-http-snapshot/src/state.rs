//! Focused tenant-state resolution for snapshot routes.

use andromeda_http_extraction::tenant_state::ExistingTenantState;
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_snapshot::service::SnapshotService;
use andromeda_tenancy::runtime::lease::RuntimeLease;
use std::sync::Arc;

/// The complete snapshot capability set exposed by one tenant.
pub struct SnapshotRouteServices {
    /// Snapshot lifecycle policy.
    pub snapshots: Arc<SnapshotService>,
    /// The tenant's JSON-LD engine, which admits the user `@context` a
    /// Snapshot is created under (clause 5.5.7).
    pub engine: JsonLdEngine,
    /// Keeps the tenant runtime resident for the life of the request holding
    /// this capability set, so eviction never closes its pools mid-request.
    pub runtime_lease: RuntimeLease,
}

impl SnapshotRouteServices {
    /// Construct focused snapshot capabilities.
    #[must_use]
    pub const fn new(snapshots: Arc<SnapshotService>, engine: JsonLdEngine, runtime_lease: RuntimeLease) -> SnapshotRouteServices {
        SnapshotRouteServices {
            snapshots,
            engine,
            runtime_lease,
        }
    }
}

/// Existing-tenant extraction specialized to snapshot capabilities.
pub(crate) type SnapshotTenantState = ExistingTenantState<Arc<SnapshotRouteServices>>;
