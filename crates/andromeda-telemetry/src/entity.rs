//! Operation accounting for the entity service.

use crate::{
    names::{ENTITY_OPERATION_SECONDS, ENTITY_OPERATIONS_TOTAL},
    operation::OperationTimer,
};
use strum::IntoStaticStr;

/// One entity-service operation, as it appears in the `operation` label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum EntityOperation {
    /// Create one entity.
    Create,
    /// Merge attributes into one entity (clause 5.6.4).
    Merge,
    /// Replace one entity's attributes (clause 5.6.3).
    Replace,
    /// Delete one entity.
    Delete,
    /// Retrieve one entity by identifier.
    Retrieve,
    /// Query entities.
    Query,
    /// Batch create (clause 5.6.7).
    BatchCreate,
    /// Batch upsert (clause 5.6.8).
    BatchUpsert,
    /// Batch update (clause 5.6.9).
    BatchUpdate,
    /// Batch merge (clause 5.6.17).
    BatchMerge,
    /// Batch delete (clause 5.6.10).
    BatchDelete,
}

/// Start timing an entity-service `operation`.
#[must_use]
pub fn start(operation: EntityOperation) -> OperationTimer {
    OperationTimer::start(ENTITY_OPERATIONS_TOTAL, ENTITY_OPERATION_SECONDS, operation.into())
}
