//! Composition of one tenant's entity spatial index for the configured
//! consistency mode.

use crate::tenant_factory::{assembly_invariant::AssemblyInvariant, error::TenantAssemblyError};
use andromeda_configuration::schema::spatial::SpatialConsistency;
use andromeda_context_commit::committer::ContextCommitter;
use andromeda_entity::{
    port::spatial_snapshot::EntitySpatialSnapshotSource,
    spatial_acceleration::SpatialAcceleration,
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
    // clone: the placement owns its tenant
    LocationPlacement::new(tenant.clone()).map_err(|error| AssemblyInvariant::LocationPlacement(Box::new(error)).into())
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::tenant_factory::spatial::{indexed_commits, initialize_spatial};
    use andromeda_configuration::schema::spatial::SpatialConfig;
    use andromeda_context_commit::{
        atomic_write::AtomicWrite,
        commit_replay::CommitReplay,
        committer::{CommitFuture, ContextCommitter},
    };
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_entity::{
        error::EntityError,
        port::spatial_snapshot::{ChangeHorizon, EntitySpatialSnapshotSource, LocationRead},
        spatial_acceleration::SpatialAcceleration,
    };
    use andromeda_ngsi_ld_lexicon::time::system_instant::SystemInstant;
    use andromeda_tenancy::tenant_ref::TenantRef;
    use futures::future::BoxFuture;
    use std::sync::Arc;

    /// A store holding no located entity.
    struct EmptyStore;

    impl EntitySpatialSnapshotSource for EmptyStore {
        fn locations<'life0, 'async_trait>(&'life0 self) -> BoxFuture<'async_trait, Result<LocationRead, EntityError>>
        where
            'life0: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async {
                Ok(LocationRead {
                    locations: Vec::new(),
                    horizon: ChangeHorizon::new(1),
                })
            })
        }

        fn locations_changed_since<'life0, 'async_trait>(&'life0 self, horizon: ChangeHorizon) -> BoxFuture<'async_trait, Result<LocationRead, EntityError>>
        where
            'life0: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async move {
                Ok(LocationRead {
                    locations: Vec::new(),
                    horizon,
                })
            })
        }
    }

    /// A commit port no test commits through.
    struct UnusedCommitter;

    impl ContextCommitter for UnusedCommitter {
        fn commit(&self, _target: &ContextTarget, _accepted_at: SystemInstant, _writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn commit_enqueued(&self, _accepted_at: SystemInstant, _writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn commit_replayed(&self, _target: &ContextTarget, _replay: CommitReplay, _writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
            Box::pin(async { Ok(Vec::new()) })
        }
    }

    /// The default composition catches the index up from the store before it
    /// narrows, which stays exact when replicas share the database, and so
    /// leaves the commit path undecorated.
    #[tokio::test]
    async fn the_default_configuration_composes_a_watermarked_index_and_plain_commits() {
        let source: Arc<dyn EntitySpatialSnapshotSource> = Arc::new(EmptyStore);
        let spatial = initialize_spatial(SpatialConfig::default().consistency, &source, &TenantRef::UnnamedDefault)
            .await
            .expect("the default composes");
        assert!(matches!(spatial, SpatialAcceleration::Watermark(_)), "the default is the watermark mode");

        let writes: Arc<dyn ContextCommitter> = Arc::new(UnusedCommitter);
        // clone: the composition is handed one handle and the test keeps another to compare
        let composed = indexed_commits(Arc::clone(&writes), &spatial, &TenantRef::UnnamedDefault).expect("composes");
        assert!(Arc::ptr_eq(&composed, &writes), "a watermarked index needs no commit-path maintenance");
    }
}
