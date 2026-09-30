//! The invariants temporal policy relies on, and how each one broke.
//!
//! None of these is the client's doing: each is a contract between temporal
//! policy, the ports it drives and the HTTP layer answering from it that a
//! correct broker never breaks, so a broken one is answered `InternalError`.

use crate::outcome::InvalidTemporalOutcome;
use andromeda_ngsi_ld_lexicon::reference::NgsiLdReferenceError;
use andromeda_ngsi_ld_model::expanded::{dataset_id::InstanceDatasetIdError, entity_member::ExpandedMemberError};
use chrono::OutOfRangeError;
use thiserror::Error;

/// A temporal policy invariant that did not hold.
#[derive(Debug, Error)]
pub enum TemporalInvariant {
    /// An entity the broker already admitted carries an instance whose
    /// `datasetId` no longer reads as an identifier.
    #[error("an admitted entity carries an unreadable datasetId: {0}")]
    AdmittedDatasetId(#[from] InstanceDatasetIdError),
    /// An entity the broker already admitted carries an `@id`, `@type` or member
    /// key that no longer reads as the value it stands for.
    #[error("an admitted entity carries an unreadable member: {0}")]
    AdmittedMember(#[from] ExpandedMemberError),
    /// Folding a Temporal Evolution into the current state produced no entity.
    #[error("folding a Temporal Evolution into no entity created none")]
    FoldCreatedNoEntity,
    /// A temporal intent retry delay cannot be expressed as a calendar duration.
    #[error("a temporal intent retry delay is out of range: {0}")]
    RetryDelayOutOfRange(#[source] OutOfRangeError),
    /// A temporal intent lease duration cannot be expressed as a calendar duration.
    #[error("a temporal intent lease duration is out of range: {0}")]
    LeaseOutOfRange(#[source] OutOfRangeError),
    /// The pending history of an entity is leased by another delivery owner.
    #[error("pending temporal history is leased by another delivery owner")]
    PendingHistoryLeasedElsewhere,
    /// Pending history remains after the bounded mutation barrier.
    #[error("pending temporal history remains after the bounded mutation barrier")]
    PendingHistoryRemains,
    /// A store reported an identifier outcome that cannot be completed.
    #[error("the store reported an incoherent identifier outcome: {0}")]
    IncompleteOutcome(#[source] InvalidTemporalOutcome),
    /// An attribute IRI the JSON-LD engine expanded is not an attribute name.
    #[error("an expanded attribute IRI is not an attribute name: {0}")]
    ExpandedAttributeName(#[source] NgsiLdReferenceError),
    /// A geometry the broker decoded cannot be written back as `GeoJSON`.
    #[error("a decoded geometry cannot be written back as GeoJSON: {0}")]
    GeometrySerialization(#[source] serde_json::Error),
    /// An entity operation a temporal operation depends on failed.
    #[error("an entity operation a temporal operation depends on failed")]
    EntityOperation,
    /// The horizon an Entity Map holds a temporal query to does not parse.
    #[error("the entity-map temporal horizon does not parse: {0}")]
    EntityMapHorizon(#[source] chrono::ParseError),
    /// The local temporal read reported a negative match count.
    #[error("the local temporal read reported a negative match count")]
    NegativeMatchCount,
    /// A joined retrieval came back without its root entity.
    #[error("the join left the retrieval without its root")]
    RetrievalWithoutRoot,
}
