//! Focused frozen temporal reads required by snapshot-scoped routes.

use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_query::type_selection::grammar::TypeExprRaw;
use futures::future::BoxFuture;
use serde_json::Value;
use thiserror::Error;

/// A failure while serving frozen temporal data.
#[derive(Debug, Error)]
pub enum TemporalSnapshotError {
    /// The requested snapshot or member does not exist.
    #[error("snapshot-scoped temporal resource not found: {detail}")]
    NotFound {
        /// Deliberate missing-resource detail.
        detail: String,
    },
    /// Snapshot persistence or an internal invariant failed.
    #[error("snapshot-scoped temporal read failed: {detail}")]
    Internal {
        /// Operator diagnostic detail.
        detail: String,
    },
}

/// Frozen temporal capabilities used only when `NGSILD-Snapshot` is present.
pub trait TemporalSnapshotScope: Send + Sync {
    /// Validate that a snapshot exists and is queryable.
    fn validate<'a>(&'a self, snapshot_id: &'a Uri) -> BoxFuture<'a, Result<(), TemporalSnapshotError>>;

    /// Retrieve one frozen temporal entity.
    fn entity<'a>(&'a self, snapshot_id: &'a Uri, entity_id: &'a Uri) -> BoxFuture<'a, Result<Option<Value>, TemporalSnapshotError>>;

    /// Query frozen temporal entities with focused selectors and pagination.
    fn entities<'a>(
        &'a self,
        snapshot_id: &'a Uri,
        entity_type: Option<&'a TypeExprRaw>,
        entity_ids: Option<&'a [Uri]>,
        limit: i64,
        offset: i64,
    ) -> BoxFuture<'a, Result<(Vec<Value>, i64), TemporalSnapshotError>>;
}
