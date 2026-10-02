//! Why a request could not be expanded into a written document.

use andromeda_jsonld::error::JsonLdError;
use andromeda_ngsi_ld_entity::refusal::EntityRefusal;
use thiserror::Error;

/// Why a request could not be expanded.
#[derive(Debug, Error)]
pub enum ExpansionError {
    /// The request's `@context` did not resolve or did not process, typed as
    /// [`andromeda_jsonld::engine::processor::JsonLdEngine::expand`] reports it.
    #[error(transparent)]
    Context(JsonLdError),
    /// The JSON-LD processor refused the document.
    #[error(transparent)]
    Processor(JsonLdError),
    /// The expanded document is not one the model admits as a write (clause
    /// 5.5.4).
    #[error(transparent)]
    Refused(#[from] EntityRefusal),
    /// The typed compact request did not serialize for the processor.
    #[error("the request did not serialize as JSON: {0}")]
    CompactDocument(#[source] serde_json::Error),
}
