//! The clause 6.3.17 warnings of a source's answer of entities or Temporal Evolutions that is not read in full.
//!
//! A warning names the registration and what was wrong with its answer. Why
//! in detail quotes what the source sent, so it is logged and not reported.

use crate::entity_reading::{EntityReading, TemporalEntityReading};
use andromeda_http_headers::ngsild_warning::{NgsiLdWarning, WarnCode};
use andromeda_jsonld::error::JsonLdError;
use andromeda_ngsi_ld_expansion::error::ExpansionError;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::received::{ElementRefusal, ReceivedDocumentError};
use std::error::Error as StdError;
use thiserror::Error;
use tracing::warn;

/// Why part of a source's answer is dropped, each reported as a revalidation
/// failure, its cause the source of the error.
#[derive(Debug, Error)]
pub(crate) enum SourceAnswerFailure<'a> {
    /// An element of the answer is refused by the model.
    #[error("context source {registration} returned an entity that is refused")]
    EntityRefused {
        /// The registration that answered.
        registration: &'a Identifier,
        /// Why the element is refused.
        #[source]
        refusal: ElementRefusal,
    },
    /// The answer is no document of entities.
    #[error("context source {registration} returned a payload that is no entity document")]
    EntityDocument {
        /// The registration that answered.
        registration: &'a Identifier,
        /// Why the body does not read.
        #[source]
        error: ReceivedDocumentError,
    },
    /// An entity does not read under the context it is spelled in.
    #[error("context source {registration} returned an entity that does not read")]
    EntityUnreadable {
        /// The registration that answered.
        registration: &'a Identifier,
        /// Why the entity does not read.
        #[source]
        reading: EntityReading,
    },
    /// The answer to a retrieval is an entity other than the one requested.
    #[error("context source {registration} answered the retrieval of {requested} with another entity")]
    OtherEntity {
        /// The registration that answered.
        registration: &'a Identifier,
        /// The entity the retrieval asked for.
        requested: &'a Identifier,
    },
    /// The answer to a retrieval continuing a flat join holds entities, none of them the one requested.
    #[error("context source {registration} answered the flat join of {requested} without that entity")]
    RequestedEntityMissing {
        /// The registration that answered.
        registration: &'a Identifier,
        /// The entity the retrieval asked for.
        requested: &'a Identifier,
    },
    /// The context the answer is spelled in does not resolve or process.
    #[error("context source {registration} answered under a context its entities cannot be read under")]
    AnswerContext {
        /// The registration that answered.
        registration: &'a Identifier,
        /// Why the context cannot be used.
        #[source]
        error: ExpansionError,
    },
    /// An element of the answer is a temporal entity the model refuses.
    #[error("context source {registration} returned a temporal entity that is refused")]
    TemporalEntityRefused {
        /// The registration that answered.
        registration: &'a Identifier,
        /// Why the element is refused.
        #[source]
        refusal: ElementRefusal,
    },
    /// The answer is no document of Temporal Evolutions.
    #[error("context source {registration} returned a payload that is no temporal entity document")]
    TemporalDocument {
        /// The registration that answered.
        registration: &'a Identifier,
        /// Why the body does not read.
        #[source]
        error: ReceivedDocumentError,
    },
    /// A Temporal Evolution does not read under the context it is spelled in.
    #[error("context source {registration} returned a temporal entity that does not read")]
    TemporalEntityUnreadable {
        /// The registration that answered.
        registration: &'a Identifier,
        /// Why the entity does not read.
        #[source]
        reading: TemporalEntityReading,
    },
    /// An aggregate cannot be spelled in the client's context.
    #[error("context source {registration} answered an aggregate that cannot be spelled in the client's context")]
    AggregateContext {
        /// The registration that answered.
        registration: &'a Identifier,
        /// Why the client's context cannot be processed.
        #[source]
        error: JsonLdError,
    },
}

impl SourceAnswerFailure<'_> {
    /// The warning the failure is reported as, its cause logged.
    pub(crate) fn into_warning(self) -> NgsiLdWarning {
        if let Some(cause) = StdError::source(&self) {
            warn!(failure = %self, %cause, "part of a context source's answer is dropped");
        }
        NgsiLdWarning::new(WarnCode::RevalidationFailed, self)
    }
}
