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
    spatial_watermark::SpatialWatermark,
};
use andromeda_spatial_index::entity::index::SpatialEntityIndex;
use andromeda_tenancy::tenant_ref::TenantRef;
use std::sync::Arc;

/// Build the tenant's spatial acceleration for `consistency` over an index no
/// rebuild has seeded yet.
///
/// Composing reads no location, so the tenant serves requests as soon as its
/// storage is assembled. An unseeded index narrows no query, so the store
/// evaluates every geo query until the rebuild task's first pass seeds the
/// index beside the tenant's requests. Under `watermark`, `catch_up` is what
/// the watermark wakes its background catch-up through.
///
/// # Errors
///
/// Returns [`TenantAssemblyError::Invariant`] when the core `location` term is
/// not a well-formed attribute name.
pub(super) fn compose_spatial(
    consistency: SpatialConsistency,
    source: &Arc<dyn EntitySpatialSnapshotSource>,
    tenant: &TenantRef,
    catch_up: Arc<dyn SpatialCatchUpSignal>,
) -> Result<SpatialAcceleration, TenantAssemblyError> {
    Ok(match consistency {
        SpatialConsistency::StoreVerified => SpatialAcceleration::StoreVerified,
        SpatialConsistency::CommitSynchronous => SpatialAcceleration::CommitSynchronous(Arc::new(SpatialEntityIndex::new())),
        SpatialConsistency::Watermark => SpatialAcceleration::Watermark(Arc::new(SpatialWatermark::new(
            Arc::new(SpatialEntityIndex::new()),
            Arc::clone(source),
            location_placement(tenant)?,
            Arc::new(TokioBlockingExecutor) as Arc<dyn BlockingExecutor>,
            catch_up,
        ))),
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
        spatial::{compose_spatial, indexed_commits},
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

    /// A store every read of which fails the test: composing reads nothing.
    struct Unread;

    impl EntitySpatialSnapshotSource for Unread {
        fn locations<'life0, 'async_trait>(&'life0 self) -> BoxFuture<'async_trait, Result<LocationRead, EntityError>>
        where
            'life0: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async { panic!("composing reads no location") })
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
            Box::pin(async { panic!("composing reads no change") })
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
            Box::pin(async { panic!("composing reads no change") })
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

    fn composed(consistency: SpatialConsistency) -> SpatialAcceleration {
        let source: Arc<dyn EntitySpatialSnapshotSource> = Arc::new(Unread);
        let (wake, _receiver) = wake_channel();
        compose_spatial(consistency, &source, &TenantRef::UnnamedDefault, spatial_catch_up_signal(wake)).expect("the configuration composes")
    }

    /// The default composition catches the index up from the store, which
    /// stays exact when replicas share the database, and counts this
    /// process's writes toward the background catch-up.
    #[test]
    fn the_default_configuration_composes_a_watermarked_index_and_catch_up_counting_commits() {
        let spatial = composed(SpatialIndexConfig::default().consistency);
        assert!(matches!(spatial, SpatialAcceleration::Watermark(_)), "the default is the watermark mode");

        let writes: Arc<dyn ContextCommitter> = Arc::new(UnusedCommitter);
        let composed = indexed_commits(Arc::clone(&writes), &spatial, &TenantRef::UnnamedDefault).expect("composes");
        assert!(!Arc::ptr_eq(&composed, &writes), "a watermarked index counts every commit");
    }

    #[test]
    fn a_store_verified_configuration_leaves_the_commit_path_undecorated() {
        let spatial = composed(SpatialConsistency::StoreVerified);
        let writes: Arc<dyn ContextCommitter> = Arc::new(UnusedCommitter);
        let composed = indexed_commits(Arc::clone(&writes), &spatial, &TenantRef::UnnamedDefault).expect("composes");
        assert!(Arc::ptr_eq(&composed, &writes), "no index needs no commit-path maintenance");
    }

    #[test]
    fn a_commit_synchronous_configuration_indexes_every_commit() {
        let spatial = composed(SpatialConsistency::CommitSynchronous);
        assert!(matches!(spatial, SpatialAcceleration::CommitSynchronous(_)));
        let writes: Arc<dyn ContextCommitter> = Arc::new(UnusedCommitter);
        let composed = indexed_commits(Arc::clone(&writes), &spatial, &TenantRef::UnnamedDefault).expect("composes");
        assert!(!Arc::ptr_eq(&composed, &writes), "a commit-synchronous index is updated by every commit");
    }

    #[test]
    fn composing_reads_no_location_in_any_mode() {
        for consistency in [
            SpatialConsistency::Watermark,
            SpatialConsistency::CommitSynchronous,
            SpatialConsistency::StoreVerified,
        ] {
            composed(consistency);
        }
    }
}
