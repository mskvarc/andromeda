//! Focused frozen-entity reads required by snapshot-scoped entity routes.

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_query::type_selection::grammar::TypeExprRaw;
use futures::future::BoxFuture;
use serde_json::Value;
use thiserror::Error;

/// Information about one entity type within a frozen snapshot.
pub type SnapshotTypeInfo = (i64, Vec<AttributeName>);

/// Information about one attribute within a frozen snapshot.
pub type SnapshotAttributeInfo = (i64, Vec<EntityTypeName>);

/// A typed failure while serving snapshot-scoped entity data.
#[derive(Debug, Error)]
pub enum EntitySnapshotError {
    /// The requested snapshot or frozen member is unavailable.
    #[error("snapshot-scoped entity resource not found: {detail}")]
    NotFound {
        /// Deliberate client-visible missing-resource detail.
        detail: String,
    },
    /// Snapshot persistence or an internal invariant failed.
    #[error("snapshot-scoped entity read failed: {detail}")]
    Internal {
        /// Operator-visible internal detail.
        detail: String,
    },
}

/// Frozen entity capabilities consumed by entity routes when `NGSILD-Snapshot` is present.
pub trait EntitySnapshotScope: Send + Sync {
    /// Validate that a snapshot exists, is unexpired, and is queryable.
    fn validate<'a>(&'a self, snapshot_id: &'a Uri) -> BoxFuture<'a, Result<(), EntitySnapshotError>>;

    /// Retrieve one frozen entity.
    fn entity<'a>(&'a self, snapshot_id: &'a Uri, entity_id: &'a Uri) -> BoxFuture<'a, Result<Option<Value>, EntitySnapshotError>>;

    /// Query frozen entities with the supplied selector and pagination.
    fn entities<'a>(
        &'a self,
        snapshot_id: &'a Uri,
        entity_type: Option<&'a TypeExprRaw>,
        entity_ids: Option<&'a [Uri]>,
        limit: i64,
        offset: i64,
    ) -> BoxFuture<'a, Result<(Vec<Value>, i64), EntitySnapshotError>>;

    /// Return distinct frozen entity types.
    fn types<'a>(&'a self, snapshot_id: &'a Uri) -> BoxFuture<'a, Result<Vec<EntityTypeName>, EntitySnapshotError>>;

    /// Return information about one frozen entity type.
    fn type_info<'a>(&'a self, snapshot_id: &'a Uri, type_name: &'a EntityTypeName) -> BoxFuture<'a, Result<Option<SnapshotTypeInfo>, EntitySnapshotError>>;

    /// Return distinct frozen attribute names.
    fn attributes<'a>(&'a self, snapshot_id: &'a Uri) -> BoxFuture<'a, Result<Vec<AttributeName>, EntitySnapshotError>>;

    /// Return information about one frozen attribute.
    fn attribute_info<'a>(
        &'a self,
        snapshot_id: &'a Uri,
        attribute_name: &'a AttributeName,
    ) -> BoxFuture<'a, Result<Option<SnapshotAttributeInfo>, EntitySnapshotError>>;
}
