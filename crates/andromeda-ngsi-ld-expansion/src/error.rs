//! Why a request could not be expanded into a written document.

use crate::assembly_error::{AssemblyError, UnspellableOrdinate};
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
    /// A geometry ordinate the broker holds has no JSON number, the broker's
    /// own fault (clause 5.5.6).
    #[error(transparent)]
    UnspellableOrdinate(#[from] UnspellableOrdinate),
    /// The typed compact request did not serialize for the processor.
    #[error("the request did not serialize as JSON: {0}")]
    CompactDocument(#[source] serde_json::Error),
}

/// Manual rather than `#[from]`: the conversion distributes the two outcomes of
/// assembly onto this enum's own variants instead of nesting them.
impl From<AssemblyError> for ExpansionError {
    fn from(error: AssemblyError) -> ExpansionError {
        match error {
            AssemblyError::Refused(refusal) => ExpansionError::Refused(refusal),
            AssemblyError::UnspellableOrdinate(fault) => ExpansionError::UnspellableOrdinate(fault),
        }
    }
}
