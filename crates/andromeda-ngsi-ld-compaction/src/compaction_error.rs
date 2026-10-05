//! Why an entity could not be compacted.

use andromeda_jsonld::error::JsonLdError;
use andromeda_ngsi_ld_model::node_refusal::RefusedNode;
use thiserror::Error;

/// A compaction that produced no compact entity.
///
/// The variants are the outcomes reading an entity through the processor has
/// always had, so a caller maps them as it mapped those.
#[derive(Debug, Error)]
pub enum CompactionError {
    /// The request context did not resolve or process, or the processor
    /// refused the document.
    #[error(transparent)]
    JsonLd(#[from] JsonLdError),
    /// The entity could not be written in its expanded form for the
    /// processor.
    #[error("the entity could not be written in expanded form: {0}")]
    Encode(#[source] serde_json::Error),
    /// The compact model refuses the compacted entity: stored data that does
    /// not read back, which is a fault of the broker, not of a request.
    #[error(transparent)]
    Refused(RefusedNode),
}
