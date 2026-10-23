//! The concrete reasons a registration request is rejected as `BadRequestData`.
//!
//! Every client-facing validation failure of registration policy is one variant
//! here, carrying the offending value as a typed field and chaining the typed
//! source error where one exists, so
//! [`RegistrationError::Validation`](crate::error::RegistrationError::Validation)
//! never holds an ad-hoc string. Each `#[error]` renders lowercase client prose;
//! spec tokens and member names (`ContextSourceRegistration`, `expiresAt`,
//! `geoproperty`, `propertyNames`, ...) keep their fixed casing, and clause
//! numbers stay in doc comments.

use crate::csr_member::CsrMember;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, reference::NgsiLdReferenceError};
use andromeda_ngsi_ld_model::context_source_info::ContextSourceInfoError;
use andromeda_ngsi_ld_request::error::ParameterError;
use iri_rs::InvalidIri;
use thiserror::Error;

/// Why a registration request is invalid.
#[derive(Debug, Error)]
pub enum RegistrationValidationReason {
    /// The `information` array was present but empty (Table 5.2.9-1).
    #[error("information must not be an empty array")]
    InformationMustNotBeEmpty,
    /// `expiresAt` was not in the future.
    #[error("expiresAt must be in the future")]
    ExpiresAtInPast,
    /// An exclusive registration block named no entities (clause 4.3.6.3).
    #[error("an exclusive registration must name the entity it covers by id")]
    ExclusiveMissingEntities,
    /// An exclusive registration named entities by something other than `id`.
    #[error("an exclusive registration must name each covered entity by id; idPattern and type-only selectors are not supported")]
    ExclusiveEntityNotById,
    /// An exclusive registration named no attributes (clause 4.3.6.3).
    #[error("an exclusive registration must name the attributes it covers in propertyNames or relationshipNames")]
    ExclusiveMissingAttributes,
    /// An auxiliary registration advertised an operation outside the permitted
    /// read set (clause 4.3.6.4).
    #[error("an auxiliary registration may only advertise retrieveOps, retrieveEntity or queryEntity")]
    AuxiliaryOperationsRestricted,
    /// A required member was absent (clause 5.2.9 cardinality 1).
    #[error("{member} is required")]
    MemberRequired {
        /// The absent member.
        member: CsrMember,
    },
    /// A member that clause 5.5.8 does not let a body delete was written as
    /// the NGSI-LD Null, or a creation body wrote a member as the Null, which
    /// clause 5.2.1 refuses outside an update.
    #[error("{member} must not be the NGSI-LD Null")]
    MemberMustNotBeNull {
        /// The member written as the NGSI-LD Null.
        member: CsrMember,
    },
    /// A creation body wrote a Context Source Property as the NGSI-LD Null,
    /// which clause 5.2.1 refuses outside an update.
    #[error("the Context Source Property {name} must not be the NGSI-LD Null in a creation request, which has nothing to delete")]
    PropertyMustNotBeNull {
        /// The property written as the NGSI-LD Null.
        name: AttributeName,
    },
    /// A name the request's `@context` expanded a term of the body to is not a
    /// name NGSI-LD can carry.
    #[error("a term of the registration expands to a name NGSI-LD cannot carry: {source}")]
    ExpandedName {
        /// Why the expanded name is not a name.
        #[source]
        source: NgsiLdReferenceError,
    },
    /// A `q` filter named an attribute that did not expand to an IRI.
    #[error("q names a non-IRI attribute: {source}")]
    QNonIriAttribute {
        /// The expansion failure.
        #[source]
        source: InvalidIri<String>,
    },
    /// A `geoproperty` selector did not expand to an IRI.
    #[error("geoproperty is not an IRI: {source}")]
    GeopropertyNotIri {
        /// The expansion failure.
        #[source]
        source: InvalidIri<String>,
    },
    /// The discovery query parameters were rejected by the parameter model.
    #[error(transparent)]
    QueryParameters(#[from] ParameterError),
    /// The `contextSourceInfo` pairs were not the shape clause 4.3.6.6 requires.
    #[error(transparent)]
    ContextSourceInfo(#[from] ContextSourceInfoError),
}
