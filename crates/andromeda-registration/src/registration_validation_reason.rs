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

use andromeda_ngsi_ld_geometry::geojson::GeometryError;
use andromeda_ngsi_ld_model::context_source_info::ContextSourceInfoError;
use andromeda_ngsi_ld_request::error::ParameterError;
use iri_rs::InvalidIri;
use thiserror::Error;

/// Why a registration request is invalid.
#[derive(Debug, Error)]
pub enum RegistrationValidationReason {
    /// The `id` member was not a URI (clause 5.2.9).
    #[error("id must be a valid URI")]
    IdNotUri,
    /// The `type` member was not `ContextSourceRegistration` (clause 5.2.9).
    #[error("type must be 'ContextSourceRegistration'")]
    TypeNotContextSourceRegistration,
    /// The `endpoint` member was explicitly null in an update.
    #[error("endpoint must not be null")]
    EndpointMustNotBeNull,
    /// The `information` member was explicitly null in an update.
    #[error("information must not be null")]
    InformationMustNotBeNull,
    /// The `information` array was present but empty (Table 5.2.9-1).
    #[error("information must not be an empty array")]
    InformationMustNotBeEmpty,
    /// `expiresAt` was not in the future.
    #[error("expiresAt must be in the future")]
    ExpiresAtInPast,
    /// A discovery query named more than one entity type (clause 5.10.2).
    #[error("CSR discovery requires a single entity type")]
    DiscoveryRequiresSingleType,
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
        member: String,
    },
    /// A member expected to be a string was not one.
    #[error("'{member}' must be a string")]
    MemberNotString {
        /// The member whose value was not a string.
        member: String,
    },
    /// A typed member was not the JSON shape its data type requires.
    #[error("invalid '{member}' value: {source}")]
    MemberValue {
        /// The member whose value was rejected.
        member: String,
        /// The JSON shape violation.
        #[source]
        source: serde_json::Error,
    },
    /// A `DateTime` member was a string outside the RFC 3339 grammar.
    #[error("invalid '{member}' value '{raw}': {source}")]
    DateTimeMember {
        /// The member whose value was rejected.
        member: String,
        /// The rejected raw value.
        raw: String,
        /// The timestamp grammar violation.
        #[source]
        source: chrono::ParseError,
    },
    /// A `Duration` member was a string outside the ISO 8601 grammar.
    ///
    /// The `iso8601_duration` parse error implements neither `Display` nor
    /// `Error`, so it cannot be chained; the rejected value is carried instead so
    /// the client still sees what it sent.
    #[error("invalid '{member}' value '{raw}'")]
    DurationMember {
        /// The member whose value was rejected.
        member: String,
        /// The rejected raw value.
        raw: String,
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
    /// A submitted `GeoProperty` geometry was not decodable `GeoJSON`.
    #[error("invalid GeoProperty geometry: {source}")]
    GeoPropertyGeometry {
        /// The geometry decoding failure.
        #[source]
        source: GeometryError,
    },
    /// A stored geometry member being carried through an update was not decodable.
    #[error("invalid existing {member} GeoJSON: {source}")]
    ExistingGeometry {
        /// The member whose stored geometry could not be read back.
        member: String,
        /// The geometry decoding failure.
        #[source]
        source: GeometryError,
    },
    /// The discovery query parameters were rejected by the parameter model.
    #[error(transparent)]
    QueryParameters(#[from] ParameterError),
    /// The `contextSourceInfo` pairs were not the shape clause 4.3.6.6 requires.
    #[error(transparent)]
    ContextSourceInfo(#[from] ContextSourceInfoError),
}
