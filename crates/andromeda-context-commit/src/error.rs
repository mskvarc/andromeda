//! Failures reported by the atomic context-commit boundary.

use andromeda_ngsi_ld_lexicon::uri::Uri;
use std::sync::Arc;
use thiserror::Error;

/// A semantic submission failure or an implementation failure hidden behind
/// the commit port.
#[derive(Debug, Error)]
pub enum CommitError {
    /// One commit submission named the same entity more than once.
    #[error("atomic write batch names {entity_id} twice; identifiers must be distinct within one commit")]
    DuplicateEntity {
        /// The repeated entity identifier.
        entity_id: Arc<Uri>,
    },
    /// An implementation could not persist or commit the write.
    #[error("atomic context commit failed: {detail}")]
    Persistence {
        /// Human-readable implementation failure detail.
        detail: String,
    },
    /// An implementation violated the commit protocol's invariants.
    #[error("atomic context commit invariant failed: {detail}")]
    Invariant {
        /// Human-readable invariant failure detail.
        detail: String,
    },
    /// A caller that submitted a known number of writes received another
    /// number of outcomes.
    #[error("atomic context commit returned {actual} outcomes; expected {expected}")]
    UnexpectedOutcomeCount {
        /// Number of outcomes required by the caller.
        expected: usize,
        /// Number of outcomes returned by the implementation.
        actual: usize,
    },
}

#[cfg(test)]
mod tests {
    use crate::error::CommitError;

    #[test]
    fn persistence_errors_remain_backend_neutral() {
        let error = CommitError::Persistence {
            detail: "connection closed".to_string(),
        };
        pretty_assertions::assert_eq!(error.to_string(), "atomic context commit failed: connection closed");
    }
}
