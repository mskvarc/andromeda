//! Which spatial index, if any, a geo query may be narrowed through.

use crate::spatial_watermark::SpatialWatermark;
use andromeda_spatial_index::entity::index::SpatialEntityIndex;
use std::sync::Arc;
use tracing::warn;

/// How the query path consults the resident spatial index, one variant per
/// [`SpatialConsistency`](andromeda_configuration::schema::entity::spatial_index::SpatialConsistency) mode.
///
/// Cloning shares the index. Every query service of a tenant narrows through
/// the one index the tenant's commits, catch-ups and rebuilds maintain.
#[derive(Clone)]
pub enum SpatialAcceleration {
    /// No index. The store evaluates every geo query.
    StoreVerified,
    /// An index every live commit updates before it returns.
    CommitSynchronous(Arc<SpatialEntityIndex>),
    /// An index caught up from the store before it narrows a query.
    Watermark(Arc<SpatialWatermark>),
}

impl SpatialAcceleration {
    /// Run `read` over the index a geo query arriving now may be narrowed
    /// through, or return `None` when the store must evaluate it in full.
    ///
    /// Under [`SpatialAcceleration::Watermark`] the index first takes in every
    /// change the store committed before the call, when they are few enough.
    /// An index behind the store, or one that cannot read it, hands the query
    /// to the store unnarrowed rather than narrowing it by stale entries.
    pub async fn narrow<R>(&self, read: impl FnOnce(&SpatialEntityIndex) -> R) -> Option<R> {
        match self {
            SpatialAcceleration::StoreVerified => None,
            SpatialAcceleration::CommitSynchronous(index) => Some(read(index)),
            SpatialAcceleration::Watermark(watermark) => watermark.narrow(read).await.unwrap_or_else(|error| {
                warn!(%error, "spatial index catch-up failed; the store evaluates the geo query unnarrowed");
                None
            }),
        }
    }
}
