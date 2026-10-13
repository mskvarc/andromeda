//! The atomic commit port and the semantics shared by its implementations.

use crate::{atomic_write::AtomicWrite, commit_replay::CommitReplay, error::CommitError, write_outcome::WriteOutcome};
use andromeda_context_space::context_target::ContextTarget;
use std::{collections::HashSet, future::Future, pin::Pin, sync::Arc};

/// Owned future returned by an object-safe atomic commit implementation.
pub type CommitFuture<'a> = Pin<Box<dyn Future<Output = Result<Vec<WriteOutcome>, CommitError>> + Send + 'a>>;

/// Persists complete semantic context writes atomically.
pub trait ContextCommitter: Send + Sync + 'static {
    /// Commit every write and return one outcome per write in submission order.
    ///
    /// The writes are independent: NGSI-LD clauses 5.6.7 through 5.6.10 permit
    /// partial batch success. Every durable consequence of one landed write is
    /// nevertheless committed in the same atomic unit as its entity mutation,
    /// under a freshly minted write identity and an ordinal counter that starts
    /// at zero.
    ///
    /// A write whose own content the store refuses is reported as
    /// [`WriteOutcome::Refused`] at its position, and every other write lands
    /// without it. A refusal the implementation cannot attribute to one write
    /// fails the whole call with
    /// [`CommitError::Submission`](crate::error::CommitError::Submission), and
    /// nothing lands.
    fn commit(&self, target: &ContextTarget, writes: Vec<AtomicWrite>) -> CommitFuture<'_>;

    /// Commit writes on the live branch for a client that asked to be answered
    /// before their consequences are produced (RFC 7240 section 4.1,
    /// `respond-async`), with the semantics of [`Self::commit`] otherwise.
    ///
    /// The writes' temporal history is committed as queued intents whichever
    /// append mode the store runs, so the drain, not the commit, writes it to
    /// the history tables; a read of the Temporal API answers it at once
    /// either way. Each write's side-effect job is queued beside it.
    fn commit_enqueued(&self, writes: Vec<AtomicWrite>) -> CommitFuture<'_>;

    /// Commit writes as the replay of a previously enqueued, partially durable
    /// commit.
    ///
    /// Every consequence lands under `replay.record`'s identity instead of a
    /// freshly minted one, and the ordinal counter that numbers its
    /// consequences resumes at
    /// `replay.base_ordinal` instead of zero, so the replay's rows belong to
    /// the write that deferred them and follow the consequences it already
    /// committed.
    ///
    /// The commit also removes the job `replay.lease` names, before it writes
    /// any consequence and only while that lease is held; a lost lease ends in
    /// [`CommitError::ReplayLeaseLost`](crate::error::CommitError::ReplayLeaseLost)
    /// with nothing written, and a job the commit removed stays out of every
    /// other worker's reach until the commit ends.
    ///
    /// A replay is one write set settling one lease, so it is never split: a
    /// refusal of any of its writes fails the whole replay with
    /// [`CommitError::Submission`](crate::error::CommitError::Submission).
    fn commit_replayed(&self, target: &ContextTarget, replay: CommitReplay, writes: Vec<AtomicWrite>) -> CommitFuture<'_>;
}

/// Reject a submission that names one entity more than once.
///
/// Fused statements cannot define an order between two mutations of the same
/// row. Requiring distinct identifiers keeps the batch a set of independent
/// per-entity atomic writes.
///
/// # Errors
///
/// Returns [`CommitError::DuplicateEntity`] carrying the repeated identifier
/// when two writes in `writes` name the same entity.
pub fn validate_submission(writes: &[AtomicWrite]) -> Result<(), CommitError> {
    let mut seen = HashSet::with_capacity(writes.len());
    for write in writes {
        if !seen.insert(write.entity_id.as_ref()) {
            return Err(CommitError::DuplicateEntity {
                entity_id: Arc::clone(&write.entity_id), // clone: the error must own the repeated identifier after validation returns.
            });
        }
    }
    Ok(())
}

/// Whether a write's durable consequences are allowed to land for an outcome.
///
/// `Skipped` is intentionally landed: an unconditional insert that found an
/// existing placeholder leaves current state untouched, while independently
/// submitted temporal observations still belong to that entity.
#[must_use]
pub const fn consequences_land(outcome: &WriteOutcome) -> bool {
    match outcome {
        WriteOutcome::Created | WriteOutcome::Updated | WriteOutcome::Deleted(_) | WriteOutcome::NoEntityChange | WriteOutcome::Skipped => true,
        WriteOutcome::AlreadyExists | WriteOutcome::NotFound | WriteOutcome::Superseded | WriteOutcome::Refused(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        atomic_write::AtomicWrite,
        committer::{consequences_land, validate_submission},
        entity_mutation::EntityMutation,
        error::CommitError,
        submission_refusal::SubmissionRefusal,
        write_outcome::WriteOutcome,
    };
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use std::sync::Arc;

    fn write(id: &str) -> AtomicWrite {
        AtomicWrite::builder()
            .entity_id(Arc::new(id.parse::<Identifier>().expect("valid entity URI")))
            .entity(EntityMutation::None)
            .build()
    }

    #[test]
    fn distinct_entity_identifiers_are_accepted() {
        let writes = [write("urn:ngsi-ld:A"), write("urn:ngsi-ld:B")];
        assert!(validate_submission(&writes).is_ok());
    }

    #[test]
    fn a_duplicate_entity_identifier_is_rejected() {
        let writes = [write("urn:ngsi-ld:A"), write("urn:ngsi-ld:A")];
        let result = validate_submission(&writes);
        assert!(matches!(result, Err(CommitError::DuplicateEntity { .. })));
    }

    #[test]
    fn only_successful_or_independent_outcomes_land_consequences() {
        for outcome in [
            WriteOutcome::Created,
            WriteOutcome::Updated,
            WriteOutcome::NoEntityChange,
            WriteOutcome::Skipped,
        ] {
            assert!(consequences_land(&outcome));
        }
        for outcome in [
            WriteOutcome::AlreadyExists,
            WriteOutcome::NotFound,
            WriteOutcome::Superseded,
            WriteOutcome::Refused(SubmissionRefusal::UnitCodeUnregistered),
        ] {
            assert!(!consequences_land(&outcome));
        }
    }
}
