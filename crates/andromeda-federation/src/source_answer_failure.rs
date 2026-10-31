//! The clause 6.3.17 warnings of a source's answer of entities or Temporal Evolutions that is not read in full.
//!
//! A warning names the registration and what was wrong with its answer. Why
//! in detail quotes what the source sent, so it is logged and not reported:
//! entities left out are counted, never named.

use andromeda_http_headers::ngsild_warning::{NgsiLdWarning, WarnCode};
use andromeda_jsonld::error::JsonLdError;
use andromeda_ngsi_ld_expansion::error::ExpansionError;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::received::ReceivedDocumentError;
use std::error::Error as StdError;
use thiserror::Error;
use tracing::warn;

/// Why part of a source's answer is dropped, each reported as a revalidation
/// failure, its cause the source of the error.
#[derive(Debug, Error)]
pub(crate) enum SourceAnswerFailure<'a> {
    /// Entities of the answer cannot be used and are left out: each is logged with its reason where it is dropped, and the source is reported once with their count.
    #[error("{count} of the entities context source {registration} answered cannot be used")]
    EntitiesDropped {
        /// The registration that answered.
        registration: &'a Identifier,
        /// How many entities are left out.
        count: usize,
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
    /// The answer is no document of Temporal Evolutions.
    #[error("context source {registration} returned a payload that is no temporal entity document")]
    TemporalDocument {
        /// The registration that answered.
        registration: &'a Identifier,
        /// Why the body does not read.
        #[source]
        error: ReceivedDocumentError,
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
