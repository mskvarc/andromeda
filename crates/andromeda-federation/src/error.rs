//! Errors produced by context-source federation.

use crate::{
    federation_conflict_reason::FederationConflictReason,
    federation_invariant::FederationInvariant,
    federation_operation_unsupported::FederationOperationUnsupported,
    gateway_failure::GatewayFailure,
};
use andromeda_jsonld::error::JsonLdError;
use andromeda_ngsi_ld_compaction::compaction_error::CompactionError;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::{error::QueryError, filter::language_targeting::LanguageTargeting};
use andromeda_registration::error::RegistrationError;
use std::time::Duration;
use thiserror::Error;

/// A failure while selecting, contacting, or combining context sources.
///
/// The four single-source variants are the clause 6.3.17 errors for an unsafe
/// method whose data lives in one exclusive or redirect registration: the
/// forwarded failure is the operation's failure, so it is reported with the
/// status and detail the clause assigns rather than folded into a 207.
#[derive(Debug, Error)]
pub enum FederationError {
    /// Federation cannot satisfy mutually exclusive registration rules.
    #[error("federation conflict: {0}")]
    Conflict(FederationConflictReason),
    /// The single registered source cannot perform the operation the request
    /// needs of it (422; clause 5.6.8.4).
    #[error("operation not supported at the registered source: {0}")]
    OperationNotSupported(FederationOperationUnsupported),
    /// The single registered source redirects back onto this broker (508).
    #[error("registration {registration} loops back onto this context broker")]
    LoopDetected {
        /// The looping registration.
        registration: Identifier,
    },
    /// The single registered source failed to respond in time (504).
    #[error("registration {registration} did not respond within {}ms", timeout.as_millis())]
    GatewayTimeout {
        /// The silent registration.
        registration: Identifier,
        /// The timeout that elapsed.
        timeout: Duration,
    },
    /// The resource was not found within the single registered source (404).
    #[error("registration {registration} holds no such resource")]
    RemoteNotFound {
        /// The registration that answered 404.
        registration: Identifier,
    },
    /// The single forwarded request failed for any other reason (502).
    #[error("registration {registration} could not complete the operation: {cause}")]
    BadGateway {
        /// The failing registration.
        registration: Identifier,
        /// The most informative cause available (clause 6.3.17): the source's
        /// own answer, or the transport failure that left none.
        ///
        /// Boxed: the cause is the widest member, and inlining it would enlarge
        /// every federation `Result`.
        #[source]
        cause: Box<GatewayFailure>,
    },
    /// Registration discovery failed.
    #[error(transparent)]
    Registration(#[from] RegistrationError),
    /// JSON-LD processing failed.
    #[error(transparent)]
    JsonLd(#[from] JsonLdError),
    /// Query processing failed.
    #[error(transparent)]
    Query(#[from] QueryError),
    /// The admitted content of a write could not be compacted under the
    /// forwarding context of a registration (clause 4.3.6.6).
    #[error(transparent)]
    Compaction(#[from] CompactionError),
    /// An upstream representation was invalid.
    #[error(transparent)]
    Representation(#[from] serde_json::Error),
    /// The write payload could not be serialised for the registration it is
    /// forwarded to, or the serialised payload not read back for the part a
    /// registration left local.
    #[error("the forwarded write payload could not be serialised or read back: {0}")]
    ForwardedBody(#[source] serde_json::Error),
    /// A `q` naming a target language a `q_target_languages` level above the
    /// specification's admits would reach a Context Source that does not
    /// advertise that level. The source refuses a full tag outright, and reads
    /// a range's wildcard by no rule at all, so no answer it gives would be the
    /// one asked for.
    #[error(
        "registration {registration} does not advertise the {needs} level of the q_target_languages extension, so the query's target languages cannot be forwarded to it"
    )]
    TargetLanguageUnadvertised {
        /// The registration naming the source.
        registration: Identifier,
        /// The least level the query's target languages need.
        needs: LanguageTargeting,
    },
    /// An internal federation invariant could not be satisfied.
    ///
    /// Boxed: the invariants carry the widest members, and inlining them would
    /// enlarge every federation `Result`. The [`From`] impl below keeps `?`
    /// ergonomic.
    #[error("federation invariant failed: {0}")]
    Internal(Box<FederationInvariant>),
}

/// Box the invariant, keeping `?` ergonomic while federation `Result`s stay small.
impl From<FederationInvariant> for FederationError {
    fn from(invariant: FederationInvariant) -> FederationError {
        FederationError::Internal(Box::new(invariant))
    }
}
