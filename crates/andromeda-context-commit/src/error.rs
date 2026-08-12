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
    /// The submitted content cannot be stored as written.
    ///
    /// A refusal the client caused and can act on — an Entity with no type
    /// (clause 4.5.1), a `unitCode` outside the closed UN/CEFACT list
    /// (clause 4.5.2), an attribute instance missing the member its type
    /// requires — as opposed to a fault of the broker. The distinction is the
    /// difference between `BadRequestData` and `InternalError` in clause 5.5.9,
    /// so it belongs on the commit port rather than in the message text.
    #[error("atomic context commit refused the submission: {detail}")]
    Submission {
        /// Human-readable detail naming what about the submission is unusable.
        detail: String,
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

    /// A refused submission reads as a statement about the request, so an
    /// operator cannot confuse it with a broker fault in a log line.
    #[test]
    fn refused_submissions_read_as_a_fault_of_the_request() {
        let error = CommitError::Submission {
            detail: "entity urn:ngsi-ld:A carries no type".to_string(),
        };
        pretty_assertions::assert_eq!(
            error.to_string(),
            "atomic context commit refused the submission: entity urn:ngsi-ld:A carries no type"
        );
    }
}
