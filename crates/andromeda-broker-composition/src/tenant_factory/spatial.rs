//! Composition of one tenant's entity spatial index for the configured
//! consistency mode.

use crate::{
    blocking_executor::TokioBlockingExecutor,
    tenant_factory::{assembly_invariant::AssemblyInvariant, error::TenantAssemblyError},
};
use andromeda_configuration::schema::entity::spatial_index::SpatialConsistency;
use andromeda_context_commit::committer::ContextCommitter;
use andromeda_entity::{
    port::{blocking_executor::BlockingExecutor, spatial_catch_up_signal::SpatialCatchUpSignal, spatial_snapshot::EntitySpatialSnapshotSource},
    spatial_acceleration::SpatialAcceleration,
    spatial_catch_up_committer::SpatialCatchUpCommitter,
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
/// failing the tenant. An unseeded index narrows no query, so the store
/// evaluates every geo query until the periodic rebuild seeds it. Under
/// `watermark`, `catch_up` is what the watermark wakes its background
/// catch-up through.
///
/// # Errors
///
/// Returns [`TenantAssemblyError::Invariant`] when the core `location` term is
/// not a well-formed attribute name.
pub(super) async fn initialize_spatial(
    consistency: SpatialConsistency,
    source: &Arc<dyn EntitySpatialSnapshotSource>,
    tenant: &TenantRef,
    catch_up: Arc<dyn SpatialCatchUpSignal>,
) -> Result<SpatialAcceleration, TenantAssemblyError> {
    Ok(match consistency {
        SpatialConsistency::StoreVerified => SpatialAcceleration::StoreVerified,
        SpatialConsistency::CommitSynchronous => {
            let index = Arc::new(SpatialEntityIndex::new());
            if let Err(error) = rebuild_location_index(&index, source.as_ref(), &location_placement(tenant)?, &TokioBlockingExecutor).await {
                tracing::warn!(%error, "spatial index seed failed; geo queries go to the store until the next rebuild");
            }
            SpatialAcceleration::CommitSynchronous(index)
        }
        SpatialConsistency::Watermark => {
            let watermark = Arc::new(SpatialWatermark::new(
                Arc::new(SpatialEntityIndex::new()),
                Arc::clone(source),
                location_placement(tenant)?,
                Arc::new(TokioBlockingExecutor) as Arc<dyn BlockingExecutor>,
                catch_up,
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
/// around every live commit, under [`SpatialAcceleration::Watermark`], one
/// that counts every live write toward the next background catch-up, and
/// otherwise `writes` itself.
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
        SpatialAcceleration::CommitSynchronous(index) => Arc::new(SpatialIndexingCommitter::new(writes, Arc::clone(index), location_placement(tenant)?)),
        SpatialAcceleration::Watermark(watermark) => Arc::new(SpatialCatchUpCommitter::new(writes, Arc::clone(watermark))),
        SpatialAcceleration::StoreVerified => writes,
    })
}

/// Where an entity's location sits in `tenant`'s index.
pub(super) fn location_placement(tenant: &TenantRef) -> Result<LocationPlacement, TenantAssemblyError> {
    LocationPlacement::new(tenant.clone()).map_err(|error| AssemblyInvariant::LocationPlacement(Box::new(error)).into())
}

#[cfg(test)]
mod tests {
    use crate::tenant_factory::{
        spatial::{indexed_commits, initialize_spatial},
        tasks::spatial_maintenance::spatial_catch_up_signal,
    };
    use andromeda_configuration::schema::entity::spatial_index::{SpatialConsistency, SpatialIndexConfig};
    use andromeda_context_commit::{
        atomic_write::AtomicWrite,
        commit_replay::CommitReplay,
        committer::{CommitFuture, ContextCommitter},
    };
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_entity::{
        error::EntityError,
        port::spatial_snapshot::{BoundedChangeRead, ChangeHorizon, EntitySpatialSnapshotSource, LocationRead},
        spatial_acceleration::SpatialAcceleration,
    };
    use andromeda_ngsi_ld_lexicon::time::system_instant::SystemInstant;
    use andromeda_task_runtime::wake_trigger::wake_channel;
    use andromeda_tenancy::tenant_ref::TenantRef;
    use futures::future::BoxFuture;
    use std::{num::NonZeroUsize, sync::Arc};

    /// A store holding no located entity.
    struct EmptyStore;

    impl EmptyStore {
        fn read() -> LocationRead {
            LocationRead {
                locations: Vec::new(),
                horizon: ChangeHorizon::new(1, Vec::new()),
            }
        }
    }

    impl EntitySpatialSnapshotSource for EmptyStore {
        fn locations<'life0, 'async_trait>(&'life0 self) -> BoxFuture<'async_trait, Result<LocationRead, EntityError>>
        where
            'life0: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async { Ok(EmptyStore::read()) })
        }

        fn locations_changed_since<'life0, 'life1, 'async_trait>(
            &'life0 self,
            _horizon: &'life1 ChangeHorizon,
        ) -> BoxFuture<'async_trait, Result<LocationRead, EntityError>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async { Ok(EmptyStore::read()) })
        }

        fn locations_changed_within<'life0, 'life1, 'async_trait>(
            &'life0 self,
            _horizon: &'life1 ChangeHorizon,
            _bound: NonZeroUsize,
        ) -> BoxFuture<'async_trait, Result<BoundedChangeRead, EntityError>>
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async { Ok(BoundedChangeRead::Within(EmptyStore::read())) })
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

    async fn composed(consistency: SpatialConsistency) -> SpatialAcceleration {
        let source: Arc<dyn EntitySpatialSnapshotSource> = Arc::new(EmptyStore);
        let (wake, _receiver) = wake_channel();
        initialize_spatial(consistency, &source, &TenantRef::UnnamedDefault, spatial_catch_up_signal(wake))
            .await
            .expect("the configuration composes")
    }

    /// The default composition catches the index up from the store, which
    /// stays exact when replicas share the database, and counts this
    /// process's writes toward the background catch-up.
    #[tokio::test]
    async fn the_default_configuration_composes_a_watermarked_index_and_catch_up_counting_commits() {
        let spatial = composed(SpatialIndexConfig::default().consistency).await;
        assert!(matches!(spatial, SpatialAcceleration::Watermark(_)), "the default is the watermark mode");

        let writes: Arc<dyn ContextCommitter> = Arc::new(UnusedCommitter);
        let composed = indexed_commits(Arc::clone(&writes), &spatial, &TenantRef::UnnamedDefault).expect("composes");
        assert!(!Arc::ptr_eq(&composed, &writes), "a watermarked index counts every commit");
    }

    #[tokio::test]
    async fn a_store_verified_configuration_leaves_the_commit_path_undecorated() {
        let spatial = composed(SpatialConsistency::StoreVerified).await;
        let writes: Arc<dyn ContextCommitter> = Arc::new(UnusedCommitter);
        let composed = indexed_commits(Arc::clone(&writes), &spatial, &TenantRef::UnnamedDefault).expect("composes");
        assert!(Arc::ptr_eq(&composed, &writes), "no index needs no commit-path maintenance");
    }

    #[tokio::test]
    async fn a_commit_synchronous_configuration_indexes_every_commit() {
        let spatial = composed(SpatialConsistency::CommitSynchronous).await;
        assert!(matches!(spatial, SpatialAcceleration::CommitSynchronous(_)));
        let writes: Arc<dyn ContextCommitter> = Arc::new(UnusedCommitter);
        let composed = indexed_commits(Arc::clone(&writes), &spatial, &TenantRef::UnnamedDefault).expect("composes");
        assert!(!Arc::ptr_eq(&composed, &writes), "a commit-synchronous index is updated by every commit");
    }
}
