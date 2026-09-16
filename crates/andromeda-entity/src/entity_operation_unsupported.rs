//! The concrete reasons the selected storage capability cannot answer an
//! operation safely.
//!
//! [`EntityError::OperationNotSupported`](crate::error::EntityError::OperationNotSupported)
//! carries one of these rather than an ad-hoc string, so every unsupported-
//! capability failure names exactly what the backend could not do.

use crate::residual_predicate::ResidualPredicate;
use thiserror::Error;

/// Why an entity operation is not supported by the selected storage capability.
#[derive(Debug, Error)]
pub enum EntityOperationUnsupported {
    /// The storage backend cannot evaluate one or more requested predicates
    /// without scanning every entity (clause 5.5.2).
    #[error("this broker's storage backend cannot evaluate {}; the request would have to be answered by reading every entity", render_predicates(.predicates))]
    Unnarrowable {
        /// The predicates the backend declined to narrow.
        predicates: Vec<ResidualPredicate>,
    },
}

/// Render a predicate list as its comma-separated NGSI-LD query-parameter names.
fn render_predicates(predicates: &[ResidualPredicate]) -> String {
    predicates.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
}
