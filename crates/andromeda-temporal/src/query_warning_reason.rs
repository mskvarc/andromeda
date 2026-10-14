//! Why one entity of a temporal query degrades the answer instead of failing it (clause 6.3.17).

use crate::error::TemporalError;
use thiserror::Error;

/// The failure a [`TemporalQueryWarning`](crate::service::TemporalQueryWarning) reports for its entity.
#[derive(Debug, Error)]
pub enum TemporalQueryWarningReason {
    /// The entity's temporal representation could not be read or rendered.
    #[error("{0}")]
    Unrenderable(#[source] TemporalError),
    /// The aggregate of the entity's merged instances could not be rendered in full.
    #[error("the merged aggregation of the entity could not be rendered in full: {0}")]
    MergedAggregation(#[source] TemporalError),
}
