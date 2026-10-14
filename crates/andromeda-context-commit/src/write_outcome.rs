//! Per-entity outcomes returned by an atomic context commit.

use crate::{deleted_entity::DeletedEntity, error::CommitError, submission_refusal::SubmissionRefusal};
use derive_more::IsVariant;

/// What one submitted [`AtomicWrite`](crate::atomic_write::AtomicWrite) did to
/// current entity state.
#[derive(Debug, IsVariant)]
pub enum WriteOutcome {
    /// A new entity row was created.
    Created,
    /// An existing entity row was updated.
    Updated,
    /// An entity row was deleted and its previous state captured.
    Deleted(DeletedEntity),
    /// A must-not-exist precondition found an existing entity.
    AlreadyExists,
    /// A required entity was absent.
    NotFound,
    /// A compare-and-set precondition observed a newer entity version.
    Superseded,
    /// An unconditional insert found an existing row and left it untouched.
    Skipped,
    /// The write carried consequences without a current-state mutation.
    NoEntityChange,
    /// The store refused the write's content, and nothing of the write landed.
    ///
    /// Clauses 5.6.7 to 5.6.10 report every entity of a batch on its own, so a
    /// refusal raised while storing one write's own content is that write's
    /// outcome, and the other writes of the commit land without it.
    Refused(SubmissionRefusal),
}

/// Extract the only outcome expected from a one-write commit.
///
/// A one-write commit whose write was refused has nothing else to report, so
/// the refusal is the commit's own failure: no caller of a one-write commit can
/// mistake [`WriteOutcome::Refused`] for a write that settled.
///
/// # Errors
///
/// Returns [`CommitError::Submission`] when the one write was refused, and
/// [`CommitError::UnexpectedOutcomeCount`] when `outcomes` holds any number of
/// outcomes other than exactly one.
pub fn single(outcomes: Vec<WriteOutcome>) -> Result<WriteOutcome, CommitError> {
    let actual = outcomes.len();
    let mut outcomes = outcomes.into_iter();
    match (outcomes.next(), outcomes.next()) {
        (Some(WriteOutcome::Refused(refusal)), None) => Err(CommitError::Submission(refusal)),
        (Some(outcome), None) => Ok(outcome),
        (None, None | Some(_)) | (Some(_), Some(_)) => Err(CommitError::UnexpectedOutcomeCount { expected: 1, actual }),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::CommitError,
        submission_refusal::SubmissionRefusal,
        write_outcome::{WriteOutcome, single},
    };

    #[test]
    fn single_returns_the_only_outcome() {
        assert!(single(vec![WriteOutcome::Created]).expect("one outcome").is_created());
    }

    #[test]
    fn single_reports_a_refused_write_as_the_commits_refusal() {
        assert!(matches!(
            single(vec![WriteOutcome::Refused(SubmissionRefusal::RelationshipMissingObject)]),
            Err(CommitError::Submission(SubmissionRefusal::RelationshipMissingObject))
        ));
    }

    #[test]
    fn single_rejects_an_empty_result() {
        assert!(matches!(
            single(Vec::new()),
            Err(CommitError::UnexpectedOutcomeCount { expected: 1, actual: 0 })
        ));
    }

    #[test]
    fn single_rejects_multiple_results() {
        assert!(matches!(
            single(vec![WriteOutcome::Created, WriteOutcome::Updated]),
            Err(CommitError::UnexpectedOutcomeCount { expected: 1, actual: 2 })
        ));
    }
}
