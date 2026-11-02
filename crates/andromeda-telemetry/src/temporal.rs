//! Operation accounting for the temporal service.

use crate::{
    names::{TEMPORAL_OPERATION_DURATION_SECONDS, TEMPORAL_OPERATIONS_TOTAL},
    operation::OperationTimer,
};
use strum::IntoStaticStr;

/// One temporal-service operation, as it appears in the `operation` label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum TemporalOperation {
    /// Create or update a temporal entity (clause 5.6.11).
    CreateOrUpdate,
    /// Add attributes to a temporal entity (clause 5.6.12).
    AddAttributes,
    /// Delete a temporal entity (clause 5.6.16).
    Delete,
    /// Delete one attribute of a temporal entity (clause 5.6.15).
    DeleteAttribute,
    /// Modify one attribute instance (clause 5.6.13).
    ModifyAttributeInstance,
    /// Delete one attribute instance (clause 5.6.14).
    DeleteAttributeInstance,
    /// Retrieve one temporal entity.
    Retrieve,
    /// Query temporal entities.
    Query,
    /// Query temporal entity identifiers.
    QueryIds,
}

/// Start timing a temporal-service `operation`.
#[must_use]
pub fn start(operation: TemporalOperation) -> OperationTimer {
    OperationTimer::start(TEMPORAL_OPERATIONS_TOTAL, TEMPORAL_OPERATION_DURATION_SECONDS, operation.into())
}
