//! The invariants an atomic commit relies on, and how each one broke.
//!
//! None of these is the client's doing: each is a contract between policy, the
//! commit port and its implementation that a correct broker never breaks, so a
//! broken one is answered `InternalError` and its detail stays in the log.

use crate::write_ordinal::WriteOrdinalOutOfRange;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::expanded::{dataset_id::InstanceDatasetIdError, entity_member::ExpandedMemberError};
use derive_more::Display;
use std::num::TryFromIntError;
use thiserror::Error;

/// A commit-protocol invariant that did not hold.
#[derive(Debug, Error)]
pub enum CommitInvariant {
    /// An entity the broker already admitted carries an instance whose
    /// `datasetId` no longer reads as an identifier.
    #[error("an admitted entity carries an unreadable datasetId: {0}")]
    AdmittedDatasetId(#[from] InstanceDatasetIdError),
    /// An entity the broker already admitted carries a type or attribute name
    /// that no longer reads as one.
    #[error("an admitted entity carries an unreadable member: {0}")]
    AdmittedMember(#[from] ExpandedMemberError),
    /// A Snapshot branch commit carried a deferred side-effect job, which only
    /// the live branch defers.
    #[error("a Snapshot branch commit carries a deferred side-effect job; only the live branch defers side effects")]
    SnapshotSideEffectJob,
    /// One write produced more consequences than a write ordinal can number.
    #[error("a write produced more consequences than an ordinal can number: {0}")]
    ConsequenceCount(#[source] TryFromIntError),
    /// Numbering a write's consequences carried the ordinal past its range.
    #[error("a write's consequence ordinal overflowed: {0}")]
    OrdinalOverflow(#[from] WriteOrdinalOutOfRange),
    /// An atomic write reached the store without the entity record it persists.
    #[error("atomic write for {entity} carries no entity record")]
    MissingEntityRecord {
        /// The entity the write addresses.
        entity: Identifier,
    },
    /// A name dictionary returned no key for a name minted in the same
    /// transaction.
    #[error("the {0} dictionary returned no key for a name minted in the same transaction")]
    UnresolvedName(NameDictionary),
    /// The store returned no outcome for one of the submitted writes.
    #[error("atomic write {index} produced no outcome")]
    MissingOutcome {
        /// Position of the write in its submission.
        index: usize,
    },
}

/// The name dictionary a commit resolves keys from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
pub enum NameDictionary {
    /// Entity type names.
    #[display("entity type")]
    EntityType,
    /// Attribute names.
    #[display("attribute name")]
    AttributeName,
}

#[cfg(test)]
mod tests {
    use crate::commit_invariant::{CommitInvariant, NameDictionary};
    use pretty_assertions::assert_eq;

    #[test]
    fn an_unresolved_name_names_its_dictionary() {
        assert_eq!(
            CommitInvariant::UnresolvedName(NameDictionary::AttributeName).to_string(),
            "the attribute name dictionary returned no key for a name minted in the same transaction"
        );
    }

    #[test]
    fn a_missing_outcome_names_the_write_position() {
        assert_eq!(CommitInvariant::MissingOutcome { index: 3 }.to_string(), "atomic write 3 produced no outcome");
    }
}
