//! The clause 6.3.17 warnings of a source's discovery answer that is not merged in full.
//!
//! A warning says what was wrong with the answer; what the source sent is
//! logged and not reported.

use andromeda_http_headers::ngsild_warning::{NgsiLdWarning, WarnCode};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, identifier::Identifier};
use thiserror::Error;
use tracing::warn;

/// Why part of a source's discovery answer is dropped, each reported as a revalidation failure.
#[derive(Debug, Error)]
pub(crate) enum DiscoveryAnswerFailure<'a> {
    /// The body does not decode as the resource asked for.
    #[error("context source {registration} answered discovery with a malformed body")]
    Malformed {
        /// The registration that answered.
        registration: &'a Identifier,
        /// Why the body does not decode.
        #[source]
        error: serde_json::Error,
    },
    /// An entity type name does not expand under the context the source spelled it in.
    #[error("a context source answered discovery with an entity type name its context does not expand")]
    UnexpandableType {
        /// The name that does not expand.
        name: &'a EntityTypeName,
    },
    /// An attribute name does not expand under the context the source spelled it in.
    #[error("a context source answered discovery with an attribute name its context does not expand")]
    UnexpandableAttribute {
        /// The name that does not expand.
        name: &'a AttributeName,
    },
}

impl DiscoveryAnswerFailure<'_> {
    /// The warning the failure is reported as, what the source sent logged.
    pub(crate) fn into_warning(self) -> NgsiLdWarning {
        match &self {
            DiscoveryAnswerFailure::Malformed { error, .. } => warn!(failure = %self, %error, "a discovery answer is not merged"),
            DiscoveryAnswerFailure::UnexpandableType { name } => warn!(failure = %self, %name, "a discovery answer is not merged in full"),
            DiscoveryAnswerFailure::UnexpandableAttribute { name } => warn!(failure = %self, %name, "a discovery answer is not merged in full"),
        }
        NgsiLdWarning::new(WarnCode::RevalidationFailed, self)
    }
}
