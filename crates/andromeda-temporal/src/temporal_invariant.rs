//! The invariants temporal policy relies on, and how each one broke.
//!
//! None of these is the client's doing: each is a contract between temporal
//! policy, the ports it drives and the HTTP layer answering from it that a
//! correct broker never breaks, so a broken one is answered `InternalError`.

use andromeda_json_literal::admission::JsonLiteralError;
use andromeda_ngsi_ld_compaction::compaction_error::CompactionError;
use andromeda_ngsi_ld_expansion::assembly_error::UnspellableOrdinate;
use andromeda_ngsi_ld_lexicon::{reference::NgsiLdReferenceError, time::date_time::NgsiLdDateTimeError};
use andromeda_ngsi_ld_render::temporal::history_row::HistoryRowRenderError;
use chrono::OutOfRangeError;
use thiserror::Error;

/// A temporal policy invariant that did not hold.
#[derive(Debug, Error)]
pub enum TemporalInvariant {
    /// A stored history row no longer reads as the instance it recorded.
    #[error("a stored history row does not render: {0}")]
    RenderedInstance(#[source] HistoryRowRenderError),
    /// A temporal entity the broker assembled could not be compacted.
    #[error("an assembled temporal entity does not compact: {0}")]
    Compaction(#[source] CompactionError),
    /// A geometry held as a Property value has an ordinate with no JSON number,
    /// although every ordinate the broker holds is finite.
    #[error("{0}")]
    UnspellableOrdinate(#[source] UnspellableOrdinate),
    /// A JSON literal merged into a stored one could not be written back.
    #[error("a merged JSON literal could not be written: {0}")]
    LiteralMerge(#[source] JsonLiteralError),
    /// Folding a Temporal Evolution into the current state produced no entity.
    #[error("folding a Temporal Evolution into no entity created none")]
    FoldCreatedNoEntity,
    /// A temporal intent retry delay cannot be expressed as a calendar duration.
    #[error("a temporal intent retry delay is out of range: {0}")]
    RetryDelayOutOfRange(#[source] OutOfRangeError),
    /// A temporal intent lease duration cannot be expressed as a calendar duration.
    #[error("a temporal intent lease duration is out of range: {0}")]
    LeaseOutOfRange(#[source] OutOfRangeError),
    /// An attribute IRI the JSON-LD engine expanded is not an attribute name.
    #[error("an expanded attribute IRI is not an attribute name: {0}")]
    ExpandedAttributeName(#[source] NgsiLdReferenceError),
    /// An entity operation a temporal operation depends on failed.
    #[error("an entity operation a temporal operation depends on failed")]
    EntityOperation,
    /// The horizon an Entity Map holds a temporal query to does not parse.
    #[error("the entity-map temporal horizon does not parse: {0}")]
    EntityMapHorizon(#[source] NgsiLdDateTimeError),
    /// The local temporal read reported a negative match count.
    #[error("the local temporal read reported a negative match count")]
    NegativeMatchCount,
    /// A joined retrieval came back without its root entity.
    #[error("the join left the retrieval without its root")]
    RetrievalWithoutRoot,
}
