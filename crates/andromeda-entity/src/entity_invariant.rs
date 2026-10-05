//! The invariants entity policy relies on, and how each one broke.
//!
//! None of these is the client's doing: each is a contract between entity
//! policy and the ports it drives that a correct broker never breaks, so a
//! broken one is answered `InternalError`.

use crate::residual_predicate::ResidualPredicate;
use andromeda_json_literal::admission::JsonLiteralError;
use andromeda_ngsi_ld_lexicon::{
    identifier::Identifier,
    ngsi_ld_urn::{AttributeListIdParseError, EntityTypeListIdParseError},
    reference::NgsiLdReferenceError,
};
use andromeda_unit_catalog::unit_code_registry::UnitCodeSnapshotMalformed;
use std::num::TryFromIntError;
use thiserror::Error;

/// An entity policy invariant that did not hold.
#[derive(Debug, Error)]
pub enum EntityInvariant {
    /// The expanded name of `scope` or of the entity `expiresAt` did not read
    /// as an attribute name, which the core vocabulary guarantees it does.
    #[error("the expanded name of an entity member is not an attribute name: {0}")]
    ReservedMemberName(#[source] NgsiLdReferenceError),
    /// An entity state could not be written in its expanded form.
    #[error("an entity state could not be written in expanded form: {0}")]
    StateEncoding(#[source] serde_json::Error),
    /// A store applied a query's result window while leaving part of its
    /// filter unapplied, so the page would be short and every later page
    /// misaligned.
    #[error("the store applied the result window but left {} unapplied", describe_predicates(.unapplied))]
    IncoherentResidual {
        /// The predicates the store left unapplied.
        unapplied: Vec<ResidualPredicate>,
    },
    /// A read-modify-write landed without the builder producing its result.
    #[error("read-modify-write for {entity} landed without producing a result")]
    ReadModifyWriteWithoutResult {
        /// The entity the write addressed.
        entity: Identifier,
    },
    /// A batch insert or update was built without the entity state it stores.
    #[error("the batch write for {entity} stores no entity state")]
    WriteWithoutStoredState {
        /// The entity the write addressed.
        entity: Identifier,
    },
    /// A JSON literal merged into a stored one could not be written back.
    #[error("a merged JSON literal could not be written: {0}")]
    LiteralMerge(#[source] JsonLiteralError),
    /// The compiled-in UN/CEFACT unit catalog did not parse, so no `unitCode`
    /// can be checked.
    #[error("the unit catalog is unreadable: {0}")]
    UnitCatalog(#[source] UnitCodeSnapshotMalformed),
    /// A durable side-effect job's attempt count left the range a retry counts in.
    #[error("a durable side-effect attempt count is out of range: {0}")]
    AttemptCountOutOfRange(#[source] TryFromIntError),
    /// A durable side-effect retry backoff left the range a timestamp can carry.
    #[error("a durable side-effect retry backoff is out of range: {0}")]
    BackoffOutOfRange(#[source] chrono::OutOfRangeError),
    /// The lock collecting a batch's prepared entries was poisoned by a panic.
    #[error("the batch preparation output lock was poisoned")]
    BatchOutputPoisoned,
    /// The executor that prepares a batch off the request task failed.
    #[error("the batch preparation executor failed")]
    BatchPreparationFailed,
    /// A blocking entity policy task panicked or was cancelled.
    #[error("a blocking entity policy task did not complete")]
    BlockingTaskFailed,
    /// The commit port returned a different number of outcomes than writes.
    #[error("a batch chunk of {submitted} writes produced {returned} outcomes")]
    OutcomeCountMismatch {
        /// Writes submitted in the chunk.
        submitted: usize,
        /// Outcomes the commit returned.
        returned: usize,
    },
    /// The core `location` term is not a well-formed attribute name.
    #[error("the default location attribute name is invalid: {0}")]
    DefaultLocationName(#[source] NgsiLdReferenceError),
    /// The identifier minted for an `EntityTypeList` (clause 5.2.24) is not one.
    #[error("a minted entity type list identifier is not one: {0}")]
    MintedEntityTypeListId(#[source] EntityTypeListIdParseError),
    /// The identifier minted for an `AttributeList` (clause 5.2.27) is not one.
    #[error("a minted attribute list identifier is not one: {0}")]
    MintedAttributeListId(#[source] AttributeListIdParseError),
}

/// Render the predicates a store left unapplied, for the invariant's message.
fn describe_predicates(predicates: &[ResidualPredicate]) -> String {
    predicates.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
}
