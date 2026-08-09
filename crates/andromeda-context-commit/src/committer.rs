//! The atomic commit port and the semantics shared by its implementations.

use crate::{atomic_write::AtomicWrite, error::CommitError, write_outcome::WriteOutcome};
use std::{collections::HashSet, future::Future, pin::Pin, sync::Arc};

/// Owned future returned by an object-safe atomic commit implementation.
pub type CommitFuture<'a> = Pin<Box<dyn Future<Output = Result<Vec<WriteOutcome>, CommitError>> + Send + 'a>>;

/// Persists complete semantic context writes atomically.
pub trait ContextCommitter: Send + Sync + 'static {
    /// Commit every write and return one outcome per write in submission order.
    ///
    /// The writes are independent: NGSI-LD clauses 5.6.7 through 5.6.10 permit
    /// partial batch success. Every durable consequence of one landed write is
    /// nevertheless committed in the same atomic unit as its entity mutation.
    fn commit(&self, writes: Vec<AtomicWrite>) -> CommitFuture<'_>;
}

/// Reject a submission that names one entity more than once.
///
/// Fused statements cannot define an order between two mutations of the same
/// row. Requiring distinct identifiers keeps the batch a set of independent
/// per-entity atomic writes.
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
pub const fn consequences_land(outcome: &WriteOutcome) -> bool {
    match outcome {
        WriteOutcome::Created | WriteOutcome::Updated | WriteOutcome::Deleted(_) | WriteOutcome::NoEntityChange | WriteOutcome::Skipped => true,
        WriteOutcome::AlreadyExists | WriteOutcome::NotFound | WriteOutcome::Superseded => false,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        atomic_write::AtomicWrite,
        committer::{consequences_land, validate_submission},
        entity_mutation::EntityMutation,
        error::CommitError,
        write_outcome::WriteOutcome,
    };
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use std::sync::Arc;

    fn write(id: &str) -> AtomicWrite {
        AtomicWrite::builder()
            .entity_id(Arc::new(id.parse::<Uri>().expect("valid entity URI")))
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
        for outcome in [WriteOutcome::AlreadyExists, WriteOutcome::NotFound, WriteOutcome::Superseded] {
            assert!(!consequences_land(&outcome));
        }
    }
}
