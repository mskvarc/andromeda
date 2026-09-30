//! The invariants entity policy relies on, and how each one broke.
//!
//! None of these is the client's doing: each is a contract between entity
//! policy and the ports it drives that a correct broker never breaks, so a
//! broken one is answered `InternalError`.

use crate::residual_predicate::ResidualPredicate;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, reference::NgsiLdReferenceError};
use andromeda_ngsi_ld_model::expanded::{dataset_id::InstanceDatasetIdError, entity_member::ExpandedMemberError};
use std::num::TryFromIntError;
use thiserror::Error;

/// An entity policy invariant that did not hold.
#[derive(Debug, Error)]
pub enum EntityInvariant {
    /// An entity the broker already admitted carries an instance whose
    /// `datasetId` no longer reads as an identifier.
    #[error("an admitted entity carries an unreadable datasetId: {0}")]
    AdmittedDatasetId(#[from] InstanceDatasetIdError),
    /// An entity the broker already admitted carries an `@id`, `@type` or member
    /// key that no longer reads as the value it stands for.
    #[error("an admitted entity carries an unreadable member: {0}")]
    AdmittedMember(#[from] ExpandedMemberError),
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
}

/// Render the predicates a store left unapplied, for the invariant's message.
fn describe_predicates(predicates: &[ResidualPredicate]) -> String {
    predicates.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
}
