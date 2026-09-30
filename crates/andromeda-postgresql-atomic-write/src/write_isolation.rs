//! Setting a refused write aside and committing the rest of its submission.
//!
//! Clauses 5.6.7 to 5.6.10 let a batch succeed for some entities and fail for
//! others, and clause 5.6.8.4 adds a `BatchEntityError` for "any kind of error
//! or abnormal situation" while processing one Entity. A commit fuses many
//! entities' writes into one transaction, so without this a refusal of one
//! write's content, raised only once the transaction is running, would roll
//! back every other write with it. The attempt that raised the refusal was
//! rolled back whole, so the submission can drop the refused write and be run
//! again under the same write record and base ordinal: nothing of the failed
//! attempt survives to collide with the next one.

use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    commit_invariant::CommitInvariant,
    error::CommitError,
    submission_refusal::SubmissionRefusal,
    write_outcome::WriteOutcome,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;

/// The writes of one submission still to be committed, and the ones set aside.
#[derive(Debug)]
pub(crate) struct IsolatedSubmission {
    /// The writes still to be committed, in submission order.
    writes: Vec<AtomicWrite>,
    /// The submission position of each write in `writes`.
    positions: Vec<usize>,
    /// Every write set aside, with its submission position.
    refused: Vec<(usize, SubmissionRefusal)>,
}

impl IsolatedSubmission {
    /// Start from the whole submission, nothing set aside.
    pub(crate) fn new(writes: Vec<AtomicWrite>) -> IsolatedSubmission {
        let positions = (0..writes.len()).collect();
        IsolatedSubmission {
            writes,
            positions,
            refused: Vec::new(),
        }
    }

    /// The writes the next attempt commits.
    pub(crate) fn writes(&self) -> &[AtomicWrite] {
        &self.writes
    }

    /// Set aside the write of `entity_id`, which the store refused.
    ///
    /// # Errors
    ///
    /// Returns [`CommitError::Submission`] when no write still to be committed
    /// names `entity_id`: a refusal that names no write of the submission
    /// cannot be isolated, so it fails the call as a whole.
    pub(crate) fn set_aside(&mut self, entity_id: &Identifier, refusal: SubmissionRefusal) -> Result<(), CommitError> {
        let Some(index) = self.writes.iter().position(|write| write.entity_id.as_ref() == entity_id) else {
            return Err(CommitError::Submission(refusal));
        };
        self.writes.remove(index);
        let position = self.positions.remove(index);
        tracing::debug!(entity = %entity_id, position, "atomic context commit set a refused write aside; committing the rest");
        self.refused.push((position, refusal));
        Ok(())
    }

    /// Every write's outcome in submission order, given the outcomes of the
    /// writes the last attempt committed.
    ///
    /// # Errors
    ///
    /// Returns [`CommitError::UnexpectedOutcomeCount`] when `landed` does not
    /// hold one outcome per write still to be committed, and
    /// [`CommitError::Invariant`] when a position was left without an outcome.
    pub(crate) fn outcomes(self, landed: Vec<WriteOutcome>) -> Result<Vec<WriteOutcome>, CommitError> {
        let IsolatedSubmission { positions, refused, .. } = self;
        if landed.len() != positions.len() {
            return Err(CommitError::UnexpectedOutcomeCount {
                expected: positions.len(),
                actual: landed.len(),
            });
        }
        // Nothing set aside: the landed outcomes already stand at their
        // submission positions, so the commit that refused nothing pays nothing.
        if refused.is_empty() {
            return Ok(landed);
        }
        let mut outcomes: Vec<Option<WriteOutcome>> = (0..positions.len() + refused.len()).map(|_| None).collect();
        let placed = positions
            .into_iter()
            .zip(landed)
            .chain(refused.into_iter().map(|(position, refusal)| (position, WriteOutcome::Refused(refusal))));
        for (position, outcome) in placed {
            if let Some(slot) = outcomes.get_mut(position) {
                *slot = Some(outcome);
            }
        }
        outcomes
            .into_iter()
            .enumerate()
            .map(|(index, outcome)| outcome.ok_or(CommitError::Invariant(CommitInvariant::MissingOutcome { index })))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::write_isolation::IsolatedSubmission;
    use andromeda_context_commit::{
        atomic_write::AtomicWrite,
        entity_mutation::EntityMutation,
        error::CommitError,
        submission_refusal::SubmissionRefusal,
        write_outcome::WriteOutcome,
    };
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use std::sync::Arc;

    fn id(raw: &str) -> Identifier {
        raw.parse().expect("a valid entity URI")
    }

    fn submission(ids: &[&str]) -> IsolatedSubmission {
        IsolatedSubmission::new(
            ids.iter()
                .map(|raw| AtomicWrite::builder().entity_id(Arc::new(id(raw))).entity(EntityMutation::None).build())
                .collect(),
        )
    }

    fn committed(submission: &IsolatedSubmission) -> Vec<&str> {
        submission.writes().iter().map(|write| write.entity_id.as_str()).collect()
    }

    #[test]
    fn a_refused_write_leaves_the_rest_to_be_committed_in_submission_order() {
        let mut submission = submission(&["urn:ngsi-ld:A", "urn:ngsi-ld:B", "urn:ngsi-ld:C"]);
        submission
            .set_aside(&id("urn:ngsi-ld:B"), SubmissionRefusal::UnitCodeUnregistered)
            .expect("B is in the submission");
        pretty_assertions::assert_eq!(committed(&submission), vec!["urn:ngsi-ld:A", "urn:ngsi-ld:C"]);
    }

    #[test]
    fn a_refused_write_reports_its_refusal_at_its_own_position() {
        let mut submission = submission(&["urn:ngsi-ld:A", "urn:ngsi-ld:B", "urn:ngsi-ld:C"]);
        submission
            .set_aside(&id("urn:ngsi-ld:B"), SubmissionRefusal::UnitCodeUnregistered)
            .expect("B is in the submission");
        let outcomes = submission
            .outcomes(vec![WriteOutcome::Created, WriteOutcome::Updated])
            .expect("one outcome per committed write");
        assert!(
            matches!(
                outcomes.as_slice(),
                [
                    WriteOutcome::Created,
                    WriteOutcome::Refused(SubmissionRefusal::UnitCodeUnregistered),
                    WriteOutcome::Updated
                ]
            ),
            "{outcomes:?}"
        );
    }

    /// Positions are the submission's, not the shrinking remainder's: a write
    /// set aside after another keeps the position the caller submitted it at.
    #[test]
    fn writes_set_aside_one_after_another_keep_their_submission_positions() {
        let mut submission = submission(&["urn:ngsi-ld:A", "urn:ngsi-ld:B", "urn:ngsi-ld:C", "urn:ngsi-ld:D"]);
        submission
            .set_aside(&id("urn:ngsi-ld:C"), SubmissionRefusal::UnitCodeMalformed)
            .expect("C is in the submission");
        submission
            .set_aside(&id("urn:ngsi-ld:A"), SubmissionRefusal::PropertyMissingValue)
            .expect("A is in the submission");
        let outcomes = submission
            .outcomes(vec![WriteOutcome::Created, WriteOutcome::Created])
            .expect("one outcome per committed write");
        assert!(
            matches!(
                outcomes.as_slice(),
                [
                    WriteOutcome::Refused(SubmissionRefusal::PropertyMissingValue),
                    WriteOutcome::Created,
                    WriteOutcome::Refused(SubmissionRefusal::UnitCodeMalformed),
                    WriteOutcome::Created
                ]
            ),
            "{outcomes:?}"
        );
    }

    /// A submission whose every write was refused commits nothing and still
    /// answers for every write.
    #[test]
    fn a_submission_whose_every_write_was_refused_reports_every_refusal() {
        let mut submission = submission(&["urn:ngsi-ld:A", "urn:ngsi-ld:B"]);
        for raw in ["urn:ngsi-ld:B", "urn:ngsi-ld:A"] {
            submission
                .set_aside(&id(raw), SubmissionRefusal::UnitCodeUnregistered)
                .expect("the write is in the submission");
        }
        assert!(submission.writes().is_empty());
        let outcomes = submission.outcomes(Vec::new()).expect("no write left to commit");
        assert!(outcomes.iter().all(WriteOutcome::is_refused), "{outcomes:?}");
        pretty_assertions::assert_eq!(outcomes.len(), 2);
    }

    #[test]
    fn a_refusal_naming_no_write_of_the_submission_fails_the_call() {
        let mut submission = submission(&["urn:ngsi-ld:A"]);
        let refused = submission.set_aside(&id("urn:ngsi-ld:Z"), SubmissionRefusal::UnitCodeUnregistered);
        assert!(
            matches!(refused, Err(CommitError::Submission(SubmissionRefusal::UnitCodeUnregistered))),
            "{refused:?}"
        );
        pretty_assertions::assert_eq!(committed(&submission), vec!["urn:ngsi-ld:A"]);
    }

    #[test]
    fn a_submission_that_refused_nothing_returns_its_outcomes_as_they_landed() {
        let submission = submission(&["urn:ngsi-ld:A", "urn:ngsi-ld:B"]);
        let outcomes = submission
            .outcomes(vec![WriteOutcome::Created, WriteOutcome::Skipped])
            .expect("one outcome per committed write");
        assert!(matches!(outcomes.as_slice(), [WriteOutcome::Created, WriteOutcome::Skipped]), "{outcomes:?}");
    }

    #[test]
    fn landed_outcomes_that_do_not_match_the_committed_writes_are_an_invariant_failure() {
        let submission = submission(&["urn:ngsi-ld:A", "urn:ngsi-ld:B"]);
        let outcomes = submission.outcomes(vec![WriteOutcome::Created]);
        assert!(
            matches!(outcomes, Err(CommitError::UnexpectedOutcomeCount { expected: 2, actual: 1 })),
            "{outcomes:?}"
        );
    }
}
