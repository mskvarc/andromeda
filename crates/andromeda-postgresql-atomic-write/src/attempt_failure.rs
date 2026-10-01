//! How one attempt at a commit transaction failed.
//!
//! Clauses 5.6.7 to 5.6.10 report every entity of a batch on its own, so a
//! refusal raised while storing one write's own content belongs to that write
//! and not to the writes sharing its transaction. The attempt that raised it is
//! rolled back whole, which leaves the committer free to set the refused write
//! aside and run the rest again ([`crate::write_isolation`]). Only an attempt
//! knows which write it was storing when it failed, so the attribution is made
//! at the phase that raised the refusal and carried out to the committer here,
//! never reconstructed from the refusal afterwards.

use andromeda_context_commit::{error::CommitError, submission_refusal::SubmissionRefusal};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use derive_more::From;

/// How one commit attempt failed.
#[derive(Debug, From)]
pub(crate) enum AttemptFailure {
    /// The content of one write was refused; the other writes are not at fault.
    /// Boxed because a refusal is the rare outcome of an attempt.
    Refused(Box<RefusedWrite>),
    /// The attempt failed as a whole.
    Failed(CommitError),
}

/// A refusal attributed to the write of one entity.
#[derive(Debug)]
pub(crate) struct RefusedWrite {
    /// The entity whose write was refused.
    pub(crate) entity_id: Identifier,
    /// Why its content cannot be stored.
    pub(crate) refusal: SubmissionRefusal,
}

impl AttemptFailure {
    /// Attribute a failure raised while storing `entity_id`'s own content to
    /// that entity's write, when the failure is a refusal of the content.
    ///
    /// Anything other than a refusal is not the write's to answer for: a
    /// persistence fault, contention or a broken invariant fails the attempt as
    /// a whole, whichever write the phase was handling when it struck.
    pub(crate) fn of_write(error: CommitError, entity_id: Identifier) -> AttemptFailure {
        match error {
            CommitError::Submission(refusal) => AttemptFailure::Refused(Box::new(RefusedWrite { entity_id, refusal })),
            whole @ (CommitError::DuplicateEntity { .. }
            | CommitError::Persistence(_)
            | CommitError::StoreSaturated
            | CommitError::StoreUnavailable
            | CommitError::TransactionContended
            | CommitError::ReplayLeaseLost { .. }
            | CommitError::Invariant(_)
            | CommitError::UnexpectedOutcomeCount { .. }) => AttemptFailure::Failed(whole),
        }
    }

    /// The failure as the commit port reports it for a call whose writes are
    /// never set aside, such as a replay: a refused write is the call's
    /// refusal.
    pub(crate) fn into_commit_error(self) -> CommitError {
        match self {
            AttemptFailure::Refused(refused) => CommitError::Submission(refused.refusal),
            AttemptFailure::Failed(error) => error,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::attempt_failure::AttemptFailure;
    use andromeda_context_commit::{error::CommitError, submission_refusal::SubmissionRefusal};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;

    fn entity_id() -> Identifier {
        "urn:ngsi-ld:Vehicle:1".parse().expect("a valid entity URI")
    }

    #[test]
    fn a_refusal_raised_for_one_write_is_attributed_to_that_write() {
        let failure = AttemptFailure::of_write(CommitError::Submission(SubmissionRefusal::UnitCodeUnregistered), entity_id());
        assert!(
            matches!(&failure, AttemptFailure::Refused(refused) if refused.entity_id == entity_id() && matches!(refused.refusal, SubmissionRefusal::UnitCodeUnregistered)),
            "{failure:?}"
        );
    }

    /// A fault is the store's, not the write's: setting the write aside would
    /// blame an entity for something it did not cause.
    #[test]
    fn a_fault_raised_for_one_write_fails_the_attempt_as_a_whole() {
        for error in [
            CommitError::Persistence(PersistenceFailure::Unavailable),
            CommitError::TransactionContended,
            CommitError::StoreSaturated,
        ] {
            let failure = AttemptFailure::of_write(error, entity_id());
            assert!(matches!(failure, AttemptFailure::Failed(_)), "{failure:?}");
        }
    }

    #[test]
    fn a_call_that_is_never_split_reports_a_refused_write_as_its_own_refusal() {
        let failure = AttemptFailure::of_write(CommitError::Submission(SubmissionRefusal::PropertyMissingValue), entity_id());
        assert!(matches!(
            failure.into_commit_error(),
            CommitError::Submission(SubmissionRefusal::PropertyMissingValue)
        ));
        let contended = AttemptFailure::Failed(CommitError::TransactionContended);
        assert!(matches!(contended.into_commit_error(), CommitError::TransactionContended));
    }
}
