//! Composition of one tenant's entity spatial index for the configured
//! consistency mode.

use crate::tenant_factory::error::TenantAssemblyError;
use andromeda_context_commit::committer::ContextCommitter;
use andromeda_entity::{
    port::spatial_snapshot::EntitySpatialSnapshotSource,
    spatial_acceleration::SpatialAcceleration,
    spatial_config::SpatialConsistency,
    spatial_indexing_committer::SpatialIndexingCommitter,
    spatial_placement::LocationPlacement,
    spatial_reconciliation::rebuild_location_index,
    spatial_watermark::SpatialWatermark,
};
use andromeda_spatial_index::entity::index::SpatialEntityIndex;
use andromeda_tenancy::tenant_ref::TenantRef;
use std::sync::Arc;

/// Build the tenant's spatial acceleration and seed its index from the store.
///
/// A seed that cannot read the store leaves the index unseeded rather than
/// failing the tenant: an unseeded index narrows no query, so the store
/// evaluates every geo query until the periodic rebuild seeds it.
///
/// # Errors
///
/// Returns [`TenantAssemblyError::Invariant`] when the core `location` term is
/// not a well-formed attribute name.
pub(super) async fn initialize_spatial(
    consistency: SpatialConsistency,
    source: &Arc<dyn EntitySpatialSnapshotSource>,
    tenant: &TenantRef,
) -> Result<SpatialAcceleration, TenantAssemblyError> {
    Ok(match consistency {
        SpatialConsistency::StoreVerified => SpatialAcceleration::StoreVerified,
        SpatialConsistency::CommitSynchronous => {
            let index = Arc::new(SpatialEntityIndex::new());
            if let Err(error) = rebuild_location_index(&index, source.as_ref(), &location_placement(tenant)?).await {
                tracing::warn!(%error, "spatial index seed failed; geo queries go to the store until the next rebuild");
            }
            SpatialAcceleration::CommitSynchronous(index)
        }
        SpatialConsistency::Watermark => {
            let watermark = Arc::new(SpatialWatermark::new(
                Arc::new(SpatialEntityIndex::new()),
                Arc::clone(source), // clone: the watermark reads the store the tenant's other ports share
                location_placement(tenant)?,
            ));
            if let Err(error) = watermark.rebuild().await {
                tracing::warn!(%error, "spatial index seed failed; geo queries go to the store until the next rebuild");
            }
            SpatialAcceleration::Watermark(watermark)
        }
    })
}

/// The commit port the tenant's writes go through: under
/// [`SpatialAcceleration::CommitSynchronous`], one that updates the index
/// around every live commit, and otherwise `writes` itself.
///
/// # Errors
///
/// Returns [`TenantAssemblyError::Invariant`] when the core `location` term is
/// not a well-formed attribute name.
pub(super) fn indexed_commits(
    writes: Arc<dyn ContextCommitter>,
    spatial: &SpatialAcceleration,
    tenant: &TenantRef,
) -> Result<Arc<dyn ContextCommitter>, TenantAssemblyError> {
    Ok(match spatial {
        SpatialAcceleration::CommitSynchronous(index) => Arc::new(SpatialIndexingCommitter::new(
            writes,
            Arc::clone(index), // clone: the committer and the query path share one index
            location_placement(tenant)?,
        )),
        SpatialAcceleration::StoreVerified | SpatialAcceleration::Watermark(_) => writes,
    })
}

/// Where an entity's location sits in `tenant`'s index.
pub(super) fn location_placement(tenant: &TenantRef) -> Result<LocationPlacement, TenantAssemblyError> {
    LocationPlacement::new(tenant.clone()).map_err(|error| TenantAssemblyError::invariant(error.to_string())) // clone: the placement owns its tenant
}
