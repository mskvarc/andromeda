//! Which spatial index, if any, a geo query may be narrowed through.

use crate::spatial_watermark::SpatialWatermark;
use andromeda_spatial_index::entity::index::SpatialEntityIndex;
use std::sync::Arc;
use tracing::warn;

/// How the query path consults the resident spatial index, one variant per
/// [`SpatialConsistency`](andromeda_configuration::schema::spatial::SpatialConsistency) mode.
///
/// Cloning shares the index: every query service of a tenant narrows through
/// the one index the tenant's commits and rebuilds maintain.
#[derive(Clone)]
pub enum SpatialAcceleration {
    /// No index: the store evaluates every geo query.
    StoreVerified,
    /// An index every live commit updates before it returns.
    CommitSynchronous(Arc<SpatialEntityIndex>),
    /// An index caught up from the store before it narrows a query.
    Watermark(Arc<SpatialWatermark>),
}

impl SpatialAcceleration {
    /// The index a geo query arriving now may be narrowed through, or `None`
    /// when the store must evaluate it in full.
    ///
    /// Under [`SpatialAcceleration::Watermark`] this first catches the index up
    /// with every change the store committed before the call. A catch-up that
    /// cannot read the store leaves the index behind it, so the query is then
    /// handed to the store unnarrowed rather than narrowed by a stale index.
    pub async fn index_for_query(&self) -> Option<&SpatialEntityIndex> {
        match self {
            SpatialAcceleration::StoreVerified => None,
            SpatialAcceleration::CommitSynchronous(index) => Some(index),
            SpatialAcceleration::Watermark(watermark) => match watermark.catch_up().await {
                Ok(()) => Some(watermark.index()),
                Err(error) => {
                    warn!(%error, "spatial index catch-up failed; the store evaluates the geo query unnarrowed");
                    None
                }
            },
        }
    }
}
