//! What a `context_source_registrations` row that no longer reads back as a
//! registration looks like.
//!
//! The broker only ever writes values it validated, so each variant is a column
//! holding something the broker never wrote: a restore from another release, or
//! manual surgery. Each names the column and carries the parser's own rejection.

use andromeda_ngsi_ld_lexicon::{
    context_source_pseudonym::ContextSourcePseudonymError,
    dereferenceable_uri::NotDereferenceable,
    identifier::IdentifierParseError,
    non_empty_text::NonEmptyTextError,
    scope::path::ScopePathError,
    tenant::InvalidTenantName,
};
use andromeda_ngsi_ld_query::error::QueryError;
use andromeda_postgresql_codes::registry::UnknownCompactCode;
use mediatype::MediaTypeError;
use thiserror::Error;

/// A stored registration column that does not parse back into its domain type.
#[derive(Debug, Error)]
pub enum StoredRegistrationError {
    /// The `id` column is not a URI.
    #[error("a stored registration id is not a URI: {0}")]
    Id(#[source] IdentifierParseError),
    /// The `registration_name` column is empty text.
    #[error("a stored registrationName is not non-empty text: {0}")]
    RegistrationName(#[source] NonEmptyTextError),
    /// The `description` column is empty text.
    #[error("a stored registration description is not non-empty text: {0}")]
    Description(#[source] NonEmptyTextError),
    /// The `tenant` column is not a tenant name.
    #[error("a stored registration tenant is not a tenant name: {0}")]
    Tenant(#[source] InvalidTenantName),
    /// The `information` column does not deserialize.
    #[error("a stored registration information document does not deserialize: {0}")]
    Information(#[source] serde_json::Error),
    /// One `dataset_id` element is not a dataset selector.
    #[error("a stored registration datasetId is not a dataset selector: {0}")]
    DatasetId(#[source] Box<QueryError>),
    /// One `scope` label path does not convert back to a scope.
    #[error("a stored registration scope label path is not a scope: {0}")]
    Scope(#[source] ScopePathError),
    /// The `observation_interval` column does not deserialize.
    #[error("a stored registration observationInterval does not deserialize: {0}")]
    ObservationInterval(#[source] serde_json::Error),
    /// The `management_interval` column does not deserialize.
    #[error("a stored registration managementInterval does not deserialize: {0}")]
    ManagementInterval(#[source] serde_json::Error),
    /// The `location` column does not deserialize into a geometry.
    #[error("a stored registration location does not deserialize: {0}")]
    Location(#[source] serde_json::Error),
    /// The `observation_space` column does not deserialize into a geometry.
    #[error("a stored registration observationSpace does not deserialize: {0}")]
    ObservationSpace(#[source] serde_json::Error),
    /// The `operation_space` column does not deserialize into a geometry.
    #[error("a stored registration operationSpace does not deserialize: {0}")]
    OperationSpace(#[source] serde_json::Error),
    /// The `additional_properties` column does not deserialize.
    #[error("a stored registration's Context Source Properties do not deserialize: {0}")]
    AdditionalProperties(#[source] serde_json::Error),
    /// The `endpoint` column is not a dereferenceable URI.
    #[error("a stored registration endpoint is not a dereferenceable URI: {0}")]
    Endpoint(#[source] NotDereferenceable),
    /// The `accept` column is not a media type.
    #[error("a stored registration accept member is not a media type: {0}")]
    Accept(#[source] MediaTypeError),
    /// The `mode` column holds a code the forwarding-mode registry never assigned.
    #[error("a stored registration mode is not a known code: {0}")]
    Mode(#[source] UnknownCompactCode),
    /// One `operations` element holds a code the operation registry never assigned.
    #[error("a stored registration operation is not a known code: {0}")]
    Operation(#[source] UnknownCompactCode),
    /// The `refresh_rate` column is not an ISO 8601 duration.
    #[error("a stored registration refreshRate is not an ISO 8601 duration past position {position}")]
    RefreshRate {
        /// Offset of the first character the duration grammar rejected.
        position: usize,
    },
    /// The `management` column does not deserialize.
    #[error("a stored registration management member does not deserialize: {0}")]
    Management(#[source] serde_json::Error),
    /// The `context_source_info` column does not deserialize.
    #[error("a stored registration contextSourceInfo does not deserialize: {0}")]
    ContextSourceInfo(#[source] serde_json::Error),
    /// The `jsonld_context` column is not a dereferenceable URI.
    #[error("a stored registration jsonldContext is not a dereferenceable URI: {0}")]
    JsonldContext(#[source] NotDereferenceable),
    /// The `context_source_alias` column is not a pseudonym.
    #[error("a stored registration contextSourceAlias is not a pseudonym: {0}")]
    ContextSourceAlias(#[source] ContextSourcePseudonymError),
    /// The `status` column holds a code the status registry never assigned.
    #[error("a stored registration status is not a known code: {0}")]
    Status(#[source] UnknownCompactCode),
}
