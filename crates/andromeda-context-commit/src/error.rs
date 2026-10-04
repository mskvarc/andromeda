//! Failures reported by the atomic context-commit boundary.

use crate::{commit_invariant::CommitInvariant, side_effect_job_id::SideEffectJobId, submission_refusal::SubmissionRefusal};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
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
        entity_id: Arc<Identifier>,
    },
    /// The submitted content cannot be stored as written.
    ///
    /// A refusal the client caused and can act on — an Entity with no type
    /// (clause 4.5.1), a `unitCode` outside the closed UN/CEFACT list
    /// (clause 4.5.2), an attribute instance missing the member its type
    /// requires — as opposed to a fault of the broker. The distinction is the
    /// difference between `BadRequestData` and `InternalError` in clause 5.5.2,
    /// so it belongs on the commit port rather than in the message text.
    #[error("atomic context commit refused the submission: {0}")]
    Submission(#[from] SubmissionRefusal),
    /// An implementation could not persist or commit the write.
    #[error("atomic context commit failed: {0}")]
    Persistence(#[source] PersistenceFailure),
    /// A connection pool was saturated before the commit could run.
    ///
    /// Backpressure, not a fault: no backend became free within the acquire
    /// deadline. The write is well-formed and can be retried, so it is answered
    /// 503 with `Retry-After`, never the 500 a broker malfunction earns.
    #[error("the datastore connection pool is saturated; retry shortly")]
    StoreSaturated,
    /// The datastore could not be reached, or the connection to it was lost.
    ///
    /// Not a fault of the request: nothing of it is known to have persisted, so
    /// the identical request can be sent again once the store is back. It is
    /// answered 503 with `Retry-After`, never the 500 a broker malfunction earns.
    #[error("the datastore is temporarily unavailable; retry shortly")]
    StoreUnavailable,
    /// The datastore rolled the commit transaction back to break a deadlock or
    /// a serialization conflict with a concurrent transaction.
    ///
    /// Not a fault of the write: the whole transaction was undone, so the
    /// identical write can run again unchanged. A committer re-runs it within a
    /// small budget of its own; a write that loses every attempt surfaces here
    /// and is answered 503 with `Retry-After`, never the 500 a broker
    /// malfunction earns.
    #[error("the commit was rolled back by a concurrent transaction; retry shortly")]
    TransactionContended,
    /// A replayed commit found the side-effect job it settles no longer leased
    /// to the replaying worker, and committed nothing.
    ///
    /// Not a fault: the lease lapsed before the replay could lock the job, and
    /// the job may already be another worker's, so that worker's replay is the
    /// one whose consequences land.
    #[error("side-effect job {job} is no longer leased to the replaying worker; the replay committed nothing")]
    ReplayLeaseLost {
        /// The job whose lease was lost.
        job: SideEffectJobId,
    },
    /// An implementation violated the commit protocol's invariants.
    #[error("atomic context commit invariant failed: {0}")]
    Invariant(#[from] CommitInvariant),
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

/// Manual rather than `#[from]`: the conversion classifies instead of wrapping.
/// A saturated store, a transaction rolled back under contention and a store
/// that cannot be reached are conditions the client can retry, so each keeps
/// its own retryable category; only a fault becomes
/// [`CommitError::Persistence`].
impl From<PersistenceFailure> for CommitError {
    fn from(failure: PersistenceFailure) -> CommitError {
        match failure {
            PersistenceFailure::Saturated => CommitError::StoreSaturated,
            PersistenceFailure::Unavailable => CommitError::StoreUnavailable,
            PersistenceFailure::RolledBack => CommitError::TransactionContended,
            fault @ (PersistenceFailure::Misconfigured
            | PersistenceFailure::ConstraintViolated
            | PersistenceFailure::StatementRejected
            | PersistenceFailure::RecordMissing
            | PersistenceFailure::Undecodable
            | PersistenceFailure::Unencodable
            | PersistenceFailure::StatementUnbuildable
            | PersistenceFailure::SchemaMismatch
            | PersistenceFailure::ContractBroken) => CommitError::Persistence(fault),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{error::CommitError, submission_refusal::SubmissionRefusal};
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;

    #[test]
    fn persistence_errors_remain_backend_neutral() {
        let error = CommitError::from(PersistenceFailure::Undecodable);
        pretty_assertions::assert_eq!(error.to_string(), "atomic context commit failed: a stored value could not be decoded");
    }

    #[test]
    fn backpressure_keeps_its_retryable_categories() {
        assert!(matches!(CommitError::from(PersistenceFailure::Saturated), CommitError::StoreSaturated));
        assert!(matches!(CommitError::from(PersistenceFailure::RolledBack), CommitError::TransactionContended));
    }

    /// A refused submission reads as a statement about the request, so an
    /// operator cannot confuse it with a broker fault in a log line.
    #[test]
    fn refused_submissions_read_as_a_fault_of_the_request() {
        let error = CommitError::from(SubmissionRefusal::UntypedEntityUnidentified);
        pretty_assertions::assert_eq!(
            error.to_string(),
            "atomic context commit refused the submission: an entity write carried no type, but at least one is required"
        );
    }

    /// A store that cannot be reached is retryable, answered 503 rather than
    /// the 500 of a fault; a configuration no retry mends stays a fault.
    #[test]
    fn an_unreachable_store_keeps_its_retryable_category() {
        assert!(matches!(CommitError::from(PersistenceFailure::Unavailable), CommitError::StoreUnavailable));
        assert!(matches!(
            CommitError::from(PersistenceFailure::Misconfigured),
            CommitError::Persistence(PersistenceFailure::Misconfigured)
        ));
    }
}
