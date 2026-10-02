//! The invariants an atomic commit relies on, and how each one broke.
//!
//! None of these is the client's doing: each is a contract between policy, the
//! commit port and its implementation that a correct broker never breaks, so a
//! broken one is answered `InternalError` and its detail stays in the log.

use crate::write_ordinal::WriteOrdinalOutOfRange;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, reference::NgsiLdReferenceError};
use derive_more::Display;
use std::num::TryFromIntError;
use thiserror::Error;

/// A commit-protocol invariant that did not hold.
#[derive(Debug, Error)]
pub enum CommitInvariant {
    /// The expanded name of a member NGSI-LD reserves on an entity (`scope`,
    /// the entity's `expiresAt`) did not read as the attribute name the search
    /// projection lists it under.
    #[error("the expanded name of a reserved entity member does not read as an attribute name: {0}")]
    ReservedMemberName(#[source] NgsiLdReferenceError),
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
