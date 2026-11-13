//! Snapshot URI resolution for context-space-aware HTTP bindings.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_http_extraction::context_target::{ContextTargetResolutionError, SnapshotTargetResolver};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use andromeda_snapshot::{error::SnapshotError, service::SnapshotService};
use futures::future::BoxFuture;
use std::sync::Arc;

/// Resolves a Snapshot URI to the branch owned by its durable control resource.
pub(super) struct BrokerSnapshotTargetResolver {
    /// The tenant's Snapshot service, whose control records map a Snapshot to
    /// its branch.
    snapshots: Arc<SnapshotService>,
}

impl BrokerSnapshotTargetResolver {
    /// Construct a target resolver over one tenant's Snapshot service.
    pub(super) const fn new(snapshots: Arc<SnapshotService>) -> BrokerSnapshotTargetResolver {
        BrokerSnapshotTargetResolver { snapshots }
    }
}

impl SnapshotTargetResolver for BrokerSnapshotTargetResolver {
    fn resolve<'a>(&'a self, snapshot_id: &'a Identifier) -> BoxFuture<'a, Result<ContextTarget, ContextTargetResolutionError>> {
        Box::pin(async move {
            self.snapshots.resolve_context_target(snapshot_id).await.map_err(|error| match error {
                SnapshotError::NotFound { identifier } => ContextTargetResolutionError::NotFound { identifier },
                SnapshotError::PreparationTimeout { identifier } => ContextTargetResolutionError::PreparationTimeout { identifier },
                SnapshotError::StoreSaturated | SnapshotError::Contended => ContextTargetResolutionError::Saturated,
                SnapshotError::Persistence(failure) => ContextTargetResolutionError::Persistence(failure),
                SnapshotError::StoreUnavailable => ContextTargetResolutionError::Persistence(PersistenceFailure::Unavailable),
                other @ (SnapshotError::Validation(_)
                | SnapshotError::AlreadyExists { .. }
                | SnapshotError::Entity(_)
                | SnapshotError::Temporal(_)
                | SnapshotError::Federation(_)
                | SnapshotError::Request(_)
                | SnapshotError::Representation(_)
                | SnapshotError::Internal(_)) => {
                    tracing::error!(error = %other, "Snapshot target resolution failed");
                    ContextTargetResolutionError::Internal
                }
            })
        })
    }
}
