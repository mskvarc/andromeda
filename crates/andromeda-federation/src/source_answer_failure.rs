//! The clause 6.3.17 warnings of a source's answer of entities or Temporal Evolutions that is not read in full.

use crate::entity_reading::{EntityReading, TemporalEntityReading};
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_jsonld::error::JsonLdError;
use andromeda_ngsi_ld_expansion::error::ExpansionError;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::received::{ElementRefusal, ReceivedDocumentError};
use thiserror::Error;

/// Why part of a source's answer is dropped, each reported as a revalidation failure.
#[derive(Debug, Error)]
pub(crate) enum SourceAnswerFailure<'a> {
    /// An element of the answer is refused by the model.
    #[error("context source {registration} returned an entity that is refused: {refusal}")]
    EntityRefused {
        /// The registration that answered.
        registration: &'a Identifier,
        /// Why the element is refused.
        refusal: ElementRefusal,
    },
    /// The answer is no document of entities.
    #[error("context source {registration} returned a payload that is no entity document: {error}")]
    EntityDocument {
        /// The registration that answered.
        registration: &'a Identifier,
        /// Why the body does not read.
        #[source]
        error: ReceivedDocumentError,
    },
    /// An entity does not read under the context it is spelled in.
    #[error("context source {registration} returned an entity that does not read: {reading}")]
    EntityUnreadable {
        /// The registration that answered.
        registration: &'a Identifier,
        /// Why the entity does not read.
        reading: EntityReading,
    },
    /// The context the answer is spelled in does not resolve or process.
    #[error("context source {registration} answered under a context its entities cannot be read under: {error}")]
    AnswerContext {
        /// The registration that answered.
        registration: &'a Identifier,
        /// Why the context cannot be used.
        #[source]
        error: ExpansionError,
    },
    /// An element of the answer is a temporal entity the model refuses.
    #[error("context source {registration} returned a temporal entity that is refused: {refusal}")]
    TemporalEntityRefused {
        /// The registration that answered.
        registration: &'a Identifier,
        /// Why the element is refused.
        refusal: ElementRefusal,
    },
    /// The answer is no document of Temporal Evolutions.
    #[error("context source {registration} returned a payload that is no temporal entity document: {error}")]
    TemporalDocument {
        /// The registration that answered.
        registration: &'a Identifier,
        /// Why the body does not read.
        #[source]
        error: ReceivedDocumentError,
    },
    /// A Temporal Evolution does not read under the context it is spelled in.
    #[error("context source {registration} returned a temporal entity that does not read: {reading}")]
    TemporalEntityUnreadable {
        /// The registration that answered.
        registration: &'a Identifier,
        /// Why the entity does not read.
        reading: TemporalEntityReading,
    },
    /// An aggregate cannot be spelled in the client's context.
    #[error("context source {registration} answered an aggregate that cannot be spelled in the client's context: {error}")]
    AggregateContext {
        /// The registration that answered.
        registration: &'a Identifier,
        /// Why the client's context cannot be processed.
        #[source]
        error: JsonLdError,
    },
}

impl SourceAnswerFailure<'_> {
    /// The warning the failure is reported as.
    pub(crate) fn into_warning(self) -> NgsiLdWarning {
        NgsiLdWarning::RevalidationFailed(self.to_string())
    }
}
