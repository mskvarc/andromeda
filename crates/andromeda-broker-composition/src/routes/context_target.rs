//! Snapshot URI resolution for context-space-aware HTTP bindings.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_http_extraction::context_target::{ContextTargetResolutionError, SnapshotTargetResolver};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_snapshot::{error::SnapshotError, service::SnapshotService};
use futures::future::BoxFuture;
use std::sync::Arc;

/// Resolves a Snapshot URI to the branch owned by its durable control resource.
pub struct BrokerSnapshotTargetResolver {
    snapshots: Arc<SnapshotService>,
}

impl BrokerSnapshotTargetResolver {
    /// Construct a target resolver over one tenant's Snapshot service.
    pub const fn new(snapshots: Arc<SnapshotService>) -> BrokerSnapshotTargetResolver {
        BrokerSnapshotTargetResolver { snapshots }
    }
}

impl SnapshotTargetResolver for BrokerSnapshotTargetResolver {
    fn resolve<'a>(&'a self, snapshot_id: &'a Uri) -> BoxFuture<'a, Result<ContextTarget, ContextTargetResolutionError>> {
        Box::pin(async move {
            self.snapshots.resolve_context_target(snapshot_id).await.map_err(|error| match error {
                SnapshotError::NotFound { identifier } => ContextTargetResolutionError::NotFound { identifier },
                SnapshotError::PreparationTimeout { identifier } => ContextTargetResolutionError::PreparationTimeout { identifier },
                SnapshotError::StoreSaturated => ContextTargetResolutionError::Saturated,
                SnapshotError::Validation { .. }
                | SnapshotError::AlreadyExists { .. }
                | SnapshotError::Entity(_)
                | SnapshotError::Temporal(_)
                | SnapshotError::Federation(_)
                | SnapshotError::Request(_)
                | SnapshotError::Representation(_)
                | SnapshotError::Persistence { .. }
                | SnapshotError::Internal { .. } => ContextTargetResolutionError::Internal { detail: error.to_string() },
            })
        })
    }
}
