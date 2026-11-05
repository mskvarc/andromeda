//! Per-entity outcomes returned by an atomic context commit.

use crate::{deleted_entity::DeletedEntity, error::CommitError, store_instant::StoreInstant, submission_refusal::SubmissionRefusal};
use andromeda_ngsi_ld_lexicon::time::system_instant::SystemInstant;
use derive_more::IsVariant;

/// What one submitted [`AtomicWrite`](crate::atomic_write::AtomicWrite) did to
/// current entity state.
#[derive(Debug, IsVariant)]
pub enum WriteOutcome {
    /// A new entity row was created, in the transaction the instant was read
    /// in.
    Created(StoreInstant),
    /// An existing entity row was updated, in the transaction the instant was
    /// read in.
    Updated(StoreInstant),
    /// An entity row was deleted and its previous state captured, in the
    /// transaction the instant was read in.
    Deleted(DeletedEntity, StoreInstant),
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
    /// The Entity holds an instant later than the one the write was stamped
    /// at, and nothing of the write landed: the greatest of its current
    /// state's and its Temporal Evolution's `modifiedAt` and of the ingestion
    /// instant of its latest queued history, at least.
    ///
    /// Clause 4.8 makes `modifiedAt` the instant of the last modification, so
    /// a write stamped before an instant the Entity holds would move it
    /// backwards. The write is stamped again after the instant carried
    /// ([`AttemptInstant`](crate::attempt_instant::AttemptInstant)), in a
    /// commit of its own. A write at the Entity's latest instant is not
    /// refused.
    Predated(SystemInstant),
    /// The store refused the write's content, and nothing of the write landed.
    ///
    /// Clauses 5.6.7 to 5.6.10 report every entity of a batch on its own, so a
    /// refusal raised while storing one write's own content is that write's
    /// outcome, and the other writes of the commit land without it.
    Refused(SubmissionRefusal),
}

impl WriteOutcome {
    /// The store instant of the transaction that changed the entity, or `None`
    /// when the write changed no current state.
    #[must_use]
    pub const fn changed_at(&self) -> Option<StoreInstant> {
        match self {
            WriteOutcome::Created(at) | WriteOutcome::Updated(at) | WriteOutcome::Deleted(_, at) => Some(*at),
            WriteOutcome::AlreadyExists
            | WriteOutcome::NotFound
            | WriteOutcome::Superseded
            | WriteOutcome::Skipped
            | WriteOutcome::NoEntityChange
            | WriteOutcome::Predated(_)
            | WriteOutcome::Refused(_) => None,
        }
    }
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
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        deleted_entity::DeletedEntity,
        error::CommitError,
        store_instant::StoreInstant,
        submission_refusal::SubmissionRefusal,
        write_outcome::{WriteOutcome, single},
    };
    use andromeda_ngsi_ld_lexicon::time::system_instant::SystemInstant;
    use chrono::{TimeZone, Utc};

    fn committed() -> StoreInstant {
        StoreInstant::new(Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant"))
    }

    #[test]
    fn single_returns_the_only_outcome() {
        assert!(single(vec![WriteOutcome::Created(committed())]).expect("one outcome").is_created());
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
            single(vec![WriteOutcome::Created(committed()), WriteOutcome::Updated(committed())]),
            Err(CommitError::UnexpectedOutcomeCount { expected: 1, actual: 2 })
        ));
    }

    #[test]
    fn only_an_outcome_that_changed_current_state_carries_its_commit_instant() {
        let deleted = DeletedEntity {
            id: "urn:ngsi-ld:A".parse().expect("valid entity URI"),
            created_at: committed().get(),
            modified_at: committed().get(),
            expires_at: None,
        };
        for changed in [
            WriteOutcome::Created(committed()),
            WriteOutcome::Updated(committed()),
            WriteOutcome::Deleted(deleted, committed()),
        ] {
            pretty_assertions::assert_eq!(changed.changed_at(), Some(committed()));
        }
        for unchanged in [
            WriteOutcome::AlreadyExists,
            WriteOutcome::NotFound,
            WriteOutcome::Superseded,
            WriteOutcome::Skipped,
            WriteOutcome::NoEntityChange,
            WriteOutcome::Predated(SystemInstant::new(committed().get())),
            WriteOutcome::Refused(SubmissionRefusal::RelationshipMissingObject),
        ] {
            pretty_assertions::assert_eq!(unchanged.changed_at(), None);
        }
    }
}
