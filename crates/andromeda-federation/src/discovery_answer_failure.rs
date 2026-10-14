//! The clause 6.3.17 warnings of a source's discovery answer that is not merged in full.

use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, identifier::Identifier};
use thiserror::Error;

/// Why part of a source's discovery answer is dropped, each reported as a revalidation failure.
#[derive(Debug, Error)]
pub(crate) enum DiscoveryAnswerFailure<'a> {
    /// The body does not decode as the resource asked for.
    #[error("context source {registration} answered discovery with a malformed body: {error}")]
    Malformed {
        /// The registration that answered.
        registration: &'a Identifier,
        /// Why the body does not decode.
        #[source]
        error: serde_json::Error,
    },
    /// An entity type name does not expand under the context the source spelled it in.
    #[error("a context source answered discovery with the name '{name}', which its context does not expand")]
    UnexpandableType {
        /// The name that does not expand.
        name: &'a EntityTypeName,
    },
    /// An attribute name does not expand under the context the source spelled it in.
    #[error("a context source answered discovery with the name '{name}', which its context does not expand")]
    UnexpandableAttribute {
        /// The name that does not expand.
        name: &'a AttributeName,
    },
}

impl DiscoveryAnswerFailure<'_> {
    /// The warning the failure is reported as.
    pub(crate) fn into_warning(self) -> NgsiLdWarning {
        NgsiLdWarning::RevalidationFailed(self.to_string())
    }
}
