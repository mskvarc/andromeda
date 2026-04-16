//! Domain-centric error types for Andromeda.
//!
//! [`ErrorKind`] is a `thiserror` enum whose variants describe *what went wrong*
//! in domain terms (e.g. `EntityNotFound`, `SubscriptionValidation`).  The HTTP
//! layer decides the NGSI-LD error category and status code.
//!
//! [`AndromedaError`] wraps an [`ErrorKind`] with an optional request URI
//! (`instance`) that handlers attach for RFC 7807 Problem Details responses.

use crate::{
    domain::ngsi_ld_urn::{
        AttributeListIdParseError,
        EntityMapIdParseError,
        EntityTypeListIdParseError,
        SnapshotIdParseError,
        SnapshotNotificationIdParseError,
    },
    jsonld::Error as JsonLdError,
    query::error::QueryError,
};
use axum::http::Uri;
use confique::Error as ConfigError;
use reqwest::Error as ReqwestError;
use serde_json::Error as SerdeJsonError;
use sqlx::{Error as SqlxError, migrate::MigrateError};
use std::io::Error as IoError;
use tracing::subscriber::SetGlobalDefaultError;

/// Domain-centric error variants.
///
/// Each variant describes a specific failure mode.  The HTTP layer maps these
/// to NGSI-LD error types and status codes; the domain layer never mentions
/// HTTP concerns.
#[derive(Debug, thiserror::Error)]
pub enum ErrorKind {
    // -- Resource not found --------------------------------------------------
    /// The requested entity does not exist.
    #[error("Entity {0} not found")]
    EntityNotFound(String),

    /// The requested subscription does not exist.
    #[error("Subscription {0} not found")]
    SubscriptionNotFound(String),

    /// The requested context source registration does not exist.
    #[error("Context source registration {0} not found")]
    CsrNotFound(String),

    /// The requested snapshot does not exist.
    #[error("Snapshot {0} not found")]
    SnapshotNotFound(String),

    /// The requested JSON-LD context does not exist.
    #[error("JSON-LD context {0} not found")]
    ContextNotFound(String),

    /// The requested entity map does not exist.
    #[error("Entity map {0} not found")]
    EntityMapNotFound(String),

    /// A temporal resource (entity, attribute, or instance) was not found.
    #[error("{0}")]
    TemporalNotFound(String),

    // -- Resource already exists ---------------------------------------------
    /// An entity with the given identifier already exists.
    #[error("Entity {0} already exists")]
    EntityAlreadyExists(String),

    /// A subscription with the given identifier already exists.
    #[error("Subscription {0} already exists")]
    SubscriptionAlreadyExists(String),

    /// A context source registration with the given identifier already exists.
    #[error("Context source registration {0} already exists")]
    CsrAlreadyExists(String),

    /// A snapshot with the given identifier already exists.
    #[error("Snapshot {0} already exists")]
    SnapshotAlreadyExists(String),

    // -- Resource conflict ---------------------------------------------------
    /// A state conflict prevented the operation.
    #[error("{0}")]
    Conflict(String),

    // -- Unsupported operation -----------------------------------------------
    /// The requested operation is not supported by this broker.
    #[error("{0}")]
    OperationNotSupported(String),

    // -- JSON-LD context availability ----------------------------------------
    /// A required JSON-LD context could not be fetched or loaded.
    #[error("{0}")]
    ContextNotAvailable(String),

    // -- Tenancy -------------------------------------------------------------
    /// The requested tenant does not exist.
    #[error("Tenant '{0}' does not exist")]
    NonexistentTenant(String),

    /// Multi-tenant support is not enabled in this deployment.
    #[error("Multi-tenant support is not enabled")]
    MultiTenancyNotSupported,

    // -- JSON-LD processing --------------------------------------------------
    /// JSON-LD expansion, compaction, IRI validation, or document parsing failed.
    #[error("{0}")]
    JsonLdProcessing(String),

    // -- Domain validation ---------------------------------------------------
    /// Subscription payload validation failed.
    #[error("{0}")]
    SubscriptionValidation(String),

    /// Entity payload validation failed.
    #[error("{0}")]
    EntityValidation(String),

    /// Temporal entity or attribute validation failed.
    #[error("{0}")]
    TemporalValidation(String),

    /// Context source registration payload validation failed.
    #[error("{0}")]
    CsrValidation(String),

    /// Snapshot payload validation failed.
    #[error("{0}")]
    SnapshotValidation(String),

    /// Entity map payload validation failed.
    #[error("{0}")]
    EntityMapValidation(String),

    /// Query parameter validation failed.
    #[error("{0}")]
    QueryValidation(String),

    // -- Request structure ---------------------------------------------------
    /// The request is structurally malformed (wrong content type, invalid JSON, etc.).
    #[error("{0}")]
    MalformedRequest(String),

    // -- Parse / conversion errors -------------------------------------------
    /// A URN could not be parsed.
    #[error("Invalid URN: {0}")]
    InvalidUrn(#[source] urn::Error),

    /// A snapshot identifier has invalid NGSI-LD URN format.
    #[error("Invalid snapshot id: {0}")]
    InvalidSnapshotId(#[source] SnapshotIdParseError),

    /// A snapshot-notification identifier has invalid NGSI-LD URN format.
    #[error("Invalid snapshot notification id: {0}")]
    InvalidSnapshotNotificationId(#[source] SnapshotNotificationIdParseError),

    /// An entity-map identifier has invalid NGSI-LD URN format.
    #[error("Invalid entity map id: {0}")]
    InvalidEntityMapId(#[source] EntityMapIdParseError),

    /// An entity-type-list identifier has invalid NGSI-LD URN format.
    #[error("Invalid entity type list id: {0}")]
    InvalidEntityTypeListId(#[source] EntityTypeListIdParseError),

    /// An attribute-list identifier has invalid NGSI-LD URN format.
    #[error("Invalid attribute list id: {0}")]
    InvalidAttributeListId(#[source] AttributeListIdParseError),

    /// JSON parsing or serialisation failed.
    #[error("JSON parse error: {0}")]
    JsonParse(#[source] serde_json::Error),

    // -- Infrastructure (sanitised before client response) --------------------
    /// A database query failed.
    #[error("Database error")]
    Database(#[source] SqlxError),

    /// A database INSERT violated a unique constraint (PG 23505).
    #[error("Database unique constraint violation")]
    DatabaseUniqueViolation(#[source] SqlxError),

    /// A database query contained an invalid regular expression (PG 2201B).
    #[error("Invalid database regex")]
    DatabaseRegexError(#[source] SqlxError),

    /// A database migration failed.
    #[error("Database migration error")]
    Migration(#[from] sqlx::migrate::MigrateError),

    /// Configuration loading failed.
    #[error("Loading configuration failed")]
    Config(#[from] confique::Error),

    /// Failed to set the global tracing subscriber.
    #[error("Failed to set default tracing subscriber")]
    TracingInit(#[from] tracing::subscriber::SetGlobalDefaultError),

    /// An I/O operation failed.
    #[error("IO error")]
    Io(#[from] std::io::Error),

    /// An outbound HTTP request failed.
    #[error("External request failed: {0}")]
    ExternalRequest(#[source] reqwest::Error),

    /// An internal error that does not fit another category.
    #[error("{0}")]
    Internal(String),
}

impl ErrorKind {
    /// Returns `true` for infrastructure errors whose details must be
    /// sanitised before sending to API clients.
    pub const fn is_internal(&self) -> bool {
        matches!(
            self,
            ErrorKind::Database(_)
                | ErrorKind::Migration(_)
                | ErrorKind::Config(_)
                | ErrorKind::TracingInit(_)
                | ErrorKind::Io(_)
                | ErrorKind::ExternalRequest(_)
                | ErrorKind::Internal(_)
        )
    }

    /// Returns `true` for all "not found" variants.
    pub const fn is_not_found(&self) -> bool {
        matches!(
            self,
            ErrorKind::EntityNotFound(_)
                | ErrorKind::SubscriptionNotFound(_)
                | ErrorKind::CsrNotFound(_)
                | ErrorKind::SnapshotNotFound(_)
                | ErrorKind::ContextNotFound(_)
                | ErrorKind::EntityMapNotFound(_)
                | ErrorKind::TemporalNotFound(_)
        )
    }
}

/// Top-level application error, pairing an [`ErrorKind`] with optional
/// HTTP request context.
#[derive(Debug, thiserror::Error)]
#[error("{kind}")]
pub struct AndromedaError {
    kind: ErrorKind,
    instance: Option<Uri>,
}

impl AndromedaError {
    /// Returns a reference to the underlying error kind.
    pub const fn kind(&self) -> &ErrorKind {
        &self.kind
    }

    /// Returns the request URI associated with this error, if any.
    pub const fn instance(&self) -> Option<&Uri> {
        self.instance.as_ref()
    }

    /// Attaches a request URI for RFC 7807 `instance` field.
    pub fn with_instance(mut self, instance: Uri) -> AndromedaError {
        self.instance = Some(instance);
        self
    }

    /// Delegates to [`ErrorKind::is_internal`].
    pub const fn is_internal(&self) -> bool {
        self.kind.is_internal()
    }

    /// Delegates to [`ErrorKind::is_not_found`].
    pub const fn is_not_found(&self) -> bool {
        self.kind.is_not_found()
    }

    // -- Resource not found factories ----------------------------------------

    /// Entity with the given identifier was not found.
    pub fn entity_not_found(id: impl Into<String>) -> AndromedaError {
        ErrorKind::EntityNotFound(id.into()).into()
    }

    /// Subscription with the given identifier was not found.
    pub fn subscription_not_found(id: impl Into<String>) -> AndromedaError {
        ErrorKind::SubscriptionNotFound(id.into()).into()
    }

    /// Context source registration with the given identifier was not found.
    pub fn csr_not_found(id: impl Into<String>) -> AndromedaError {
        ErrorKind::CsrNotFound(id.into()).into()
    }

    /// Snapshot with the given identifier was not found.
    pub fn snapshot_not_found(id: impl Into<String>) -> AndromedaError {
        ErrorKind::SnapshotNotFound(id.into()).into()
    }

    /// JSON-LD context with the given identifier was not found.
    pub fn context_not_found(id: impl Into<String>) -> AndromedaError {
        ErrorKind::ContextNotFound(id.into()).into()
    }

    /// Entity map with the given identifier was not found.
    pub fn entity_map_not_found(id: impl Into<String>) -> AndromedaError {
        ErrorKind::EntityMapNotFound(id.into()).into()
    }

    /// Temporal resource (entity, attribute, or instance) was not found.
    pub fn temporal_not_found(message: impl Into<String>) -> AndromedaError {
        ErrorKind::TemporalNotFound(message.into()).into()
    }

    // -- Resource already exists factories -----------------------------------

    /// Entity with the given identifier already exists.
    pub fn entity_already_exists(id: impl Into<String>) -> AndromedaError {
        ErrorKind::EntityAlreadyExists(id.into()).into()
    }

    /// Subscription with the given identifier already exists.
    pub fn subscription_already_exists(id: impl Into<String>) -> AndromedaError {
        ErrorKind::SubscriptionAlreadyExists(id.into()).into()
    }

    /// Context source registration with the given identifier already exists.
    pub fn csr_already_exists(id: impl Into<String>) -> AndromedaError {
        ErrorKind::CsrAlreadyExists(id.into()).into()
    }

    /// Snapshot with the given identifier already exists.
    pub fn snapshot_already_exists(id: impl Into<String>) -> AndromedaError {
        ErrorKind::SnapshotAlreadyExists(id.into()).into()
    }

    // -- JSON-LD context availability ----------------------------------------

    /// A required JSON-LD context could not be fetched or loaded.
    pub fn context_not_available(message: impl Into<String>) -> AndromedaError {
        ErrorKind::ContextNotAvailable(message.into()).into()
    }

    // -- Tenancy -------------------------------------------------------------

    /// The requested tenant does not exist.
    pub fn nonexistent_tenant(name: impl Into<String>) -> AndromedaError {
        ErrorKind::NonexistentTenant(name.into()).into()
    }

    /// Multi-tenant support is not enabled.
    pub fn multi_tenancy_not_supported() -> AndromedaError {
        ErrorKind::MultiTenancyNotSupported.into()
    }

    // -- JSON-LD processing --------------------------------------------------

    /// JSON-LD processing (expansion, compaction, IRI validation) failed.
    pub fn jsonld_processing(message: impl Into<String>) -> AndromedaError {
        ErrorKind::JsonLdProcessing(message.into()).into()
    }

    // -- Domain validation factories -----------------------------------------

    /// Subscription payload validation failed.
    pub fn subscription_validation(message: impl Into<String>) -> AndromedaError {
        ErrorKind::SubscriptionValidation(message.into()).into()
    }

    /// Entity payload validation failed.
    pub fn entity_validation(message: impl Into<String>) -> AndromedaError {
        ErrorKind::EntityValidation(message.into()).into()
    }

    /// Temporal entity or attribute validation failed.
    pub fn temporal_validation(message: impl Into<String>) -> AndromedaError {
        ErrorKind::TemporalValidation(message.into()).into()
    }

    /// Context source registration payload validation failed.
    pub fn csr_validation(message: impl Into<String>) -> AndromedaError {
        ErrorKind::CsrValidation(message.into()).into()
    }

    /// Snapshot payload validation failed.
    pub fn snapshot_validation(message: impl Into<String>) -> AndromedaError {
        ErrorKind::SnapshotValidation(message.into()).into()
    }

    /// Entity map payload validation failed.
    pub fn entity_map_validation(message: impl Into<String>) -> AndromedaError {
        ErrorKind::EntityMapValidation(message.into()).into()
    }

    /// Query parameter validation failed.
    pub fn query_validation(message: impl Into<String>) -> AndromedaError {
        ErrorKind::QueryValidation(message.into()).into()
    }

    // -- Request structure ---------------------------------------------------

    /// The request is structurally malformed.
    pub fn malformed_request(message: impl Into<String>) -> AndromedaError {
        ErrorKind::MalformedRequest(message.into()).into()
    }

    // -- Infrastructure ------------------------------------------------------

    /// An internal error that does not fit another category.
    pub fn internal(message: impl Into<String>) -> AndromedaError {
        ErrorKind::Internal(message.into()).into()
    }
}

/// Extension methods for attaching HTTP request URI context to fallible results.
pub trait ResultInstanceExt<T, E> {
    /// Converts the error into [`AndromedaError`] and attaches `instance`.
    fn with_instance(self, instance: Uri) -> Result<T, AndromedaError>;
}

impl<T, E> ResultInstanceExt<T, E> for Result<T, E>
where
    E: Into<AndromedaError>,
{
    fn with_instance(self, instance: Uri) -> Result<T, AndromedaError> {
        self.map_err(|error| error.into().with_instance(instance))
    }
}

// -- From<ErrorKind> for AndromedaError --------------------------------------

impl From<ErrorKind> for AndromedaError {
    fn from(kind: ErrorKind) -> AndromedaError {
        AndromedaError { kind, instance: None }
    }
}

// -- From impls that go through ErrorKind's #[from] --------------------------

impl From<MigrateError> for AndromedaError {
    fn from(error: MigrateError) -> AndromedaError {
        ErrorKind::from(error).into()
    }
}

impl From<ConfigError> for AndromedaError {
    fn from(error: ConfigError) -> AndromedaError {
        ErrorKind::from(error).into()
    }
}

impl From<SetGlobalDefaultError> for AndromedaError {
    fn from(error: SetGlobalDefaultError) -> AndromedaError {
        ErrorKind::from(error).into()
    }
}

impl From<IoError> for AndromedaError {
    fn from(error: IoError) -> AndromedaError {
        ErrorKind::from(error).into()
    }
}

// -- Manual From impls -------------------------------------------------------

impl From<SqlxError> for AndromedaError {
    fn from(error: SqlxError) -> AndromedaError {
        let kind = if let SqlxError::Database(ref db_error) = error {
            match db_error.code().as_deref() {
                Some("23505") => ErrorKind::DatabaseUniqueViolation(error),
                Some("2201B") => ErrorKind::DatabaseRegexError(error),
                Some(_) => ErrorKind::Database(error),
                None => ErrorKind::Database(error),
            }
        } else {
            ErrorKind::Database(error)
        };
        kind.into()
    }
}

impl From<JsonLdError> for AndromedaError {
    fn from(error: JsonLdError) -> AndromedaError {
        let kind = match error {
            JsonLdError::ContextFetchDisabled { ref url } => ErrorKind::ContextNotAvailable(format!("Context fetch disabled for {}", url)),
            JsonLdError::ContextHttp { ref url, status } => ErrorKind::ContextNotAvailable(format!("HTTP {} while loading context {}", status, url)),
            JsonLdError::ContextLoad { ref url, ref message } => ErrorKind::ContextNotAvailable(format!("Failed to load context {}: {}", url, message)),
            JsonLdError::ContextTooLarge { ref url, size, max } => {
                ErrorKind::ContextNotAvailable(format!("Context {} too large ({} > {} bytes)", url, size, max))
            }
            JsonLdError::ContextInvalid { ref url, ref message } => ErrorKind::ContextNotAvailable(format!("Invalid context {}: {}", url, message)),
            JsonLdError::EmbeddedContextInvalid(ref message) => ErrorKind::JsonLdProcessing(format!("Embedded context invalid: {}", message)),
            JsonLdError::Expansion(_) | JsonLdError::Compaction(_) => ErrorKind::JsonLdProcessing(error.to_string()),
            JsonLdError::InvalidIri(_) | JsonLdError::InvalidDocument(_) => ErrorKind::JsonLdProcessing(error.to_string()),
            JsonLdError::Json(source) => ErrorKind::JsonParse(source),
        };
        kind.into()
    }
}

impl From<SerdeJsonError> for AndromedaError {
    fn from(error: SerdeJsonError) -> AndromedaError {
        ErrorKind::JsonParse(error).into()
    }
}

impl From<urn::Error> for AndromedaError {
    fn from(error: urn::Error) -> AndromedaError {
        ErrorKind::InvalidUrn(error).into()
    }
}

impl From<SnapshotIdParseError> for AndromedaError {
    fn from(error: SnapshotIdParseError) -> AndromedaError {
        ErrorKind::InvalidSnapshotId(error).into()
    }
}

impl From<SnapshotNotificationIdParseError> for AndromedaError {
    fn from(error: SnapshotNotificationIdParseError) -> AndromedaError {
        ErrorKind::InvalidSnapshotNotificationId(error).into()
    }
}

impl From<EntityMapIdParseError> for AndromedaError {
    fn from(error: EntityMapIdParseError) -> AndromedaError {
        ErrorKind::InvalidEntityMapId(error).into()
    }
}

impl From<EntityTypeListIdParseError> for AndromedaError {
    fn from(error: EntityTypeListIdParseError) -> AndromedaError {
        ErrorKind::InvalidEntityTypeListId(error).into()
    }
}

impl From<AttributeListIdParseError> for AndromedaError {
    fn from(error: AttributeListIdParseError) -> AndromedaError {
        ErrorKind::InvalidAttributeListId(error).into()
    }
}

impl From<QueryError> for AndromedaError {
    fn from(err: QueryError) -> AndromedaError {
        let kind = match &err {
            QueryError::QEmpty
            | QueryError::QUnterminatedString
            | QueryError::QUnexpectedToken { .. }
            | QueryError::QExpected { .. }
            | QueryError::QUnexpectedChar { .. }
            | QueryError::TypeExprEmpty
            | QueryError::TypeExprUnexpectedEnd
            | QueryError::TypeExprUnbalanced(_)
            | QueryError::TypeExprUnexpected { .. }
            | QueryError::TypeExprEmptyName(_)
            | QueryError::GeoIncomplete
            | QueryError::ParamOptionsInvalid(_)
            | QueryError::ParamFormatInvalid(_)
            | QueryError::ParamTimerelTimeatRequired
            | QueryError::ParamTimerelTimeatPair => ErrorKind::MalformedRequest(err.to_string()),
            QueryError::QInvalidNumber(_)
            | QueryError::IdPatternRegex(_)
            | QueryError::GeoUnknownRel(_)
            | QueryError::GeoInvalidDistance { .. }
            | QueryError::IcuCollation(_)
            | QueryError::ParamIdInvalid(_)
            | QueryError::ParamIdListEmpty
            | QueryError::ParamIdPatternEmpty
            | QueryError::ParamOrderGeometryInvalid(_)
            | QueryError::ParamOrderGeometryCollection
            | QueryError::ParamOrderFromInvalid { .. }
            | QueryError::ParamDropKeepExclusive
            | QueryError::ParamTimerelInvalid(_)
            | QueryError::ParamAggrMethodInvalid(_)
            | QueryError::ParamEndTimeAtRequired
            | QueryError::ParamOther(_) => ErrorKind::QueryValidation(err.to_string()),
        };
        kind.into()
    }
}

impl From<ReqwestError> for AndromedaError {
    fn from(error: ReqwestError) -> AndromedaError {
        ErrorKind::ExternalRequest(error).into()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use sqlx::{
        Error as SqlxError,
        error::{DatabaseError, ErrorKind as SqlxErrorKind},
    };
    use std::{borrow::Cow, error::Error as StdError, fmt};

    // -- ErrorKind Display and is_internal ------------------------------------

    #[test]
    fn entity_not_found_display() {
        let kind = ErrorKind::EntityNotFound("urn:x".into());
        pretty_assertions::assert_eq!(kind.to_string(), "Entity urn:x not found");
        assert!(!kind.is_internal());
    }

    #[test]
    fn subscription_not_found_display() {
        let kind = ErrorKind::SubscriptionNotFound("sub1".into());
        pretty_assertions::assert_eq!(kind.to_string(), "Subscription sub1 not found");
        assert!(!kind.is_internal());
    }

    #[test]
    fn entity_already_exists_display() {
        let kind = ErrorKind::EntityAlreadyExists("urn:x".into());
        pretty_assertions::assert_eq!(kind.to_string(), "Entity urn:x already exists");
        assert!(!kind.is_internal());
    }

    #[test]
    fn nonexistent_tenant_display() {
        let kind = ErrorKind::NonexistentTenant("t1".into());
        pretty_assertions::assert_eq!(kind.to_string(), "Tenant 't1' does not exist");
        assert!(!kind.is_internal());
    }

    #[test]
    fn multi_tenancy_not_supported_display() {
        let kind = ErrorKind::MultiTenancyNotSupported;
        pretty_assertions::assert_eq!(kind.to_string(), "Multi-tenant support is not enabled");
        assert!(!kind.is_internal());
    }

    #[test]
    fn internal_error_display() {
        let kind = ErrorKind::Internal("msg".into());
        pretty_assertions::assert_eq!(kind.to_string(), "msg");
        assert!(kind.is_internal());
    }

    #[test]
    fn database_error_is_internal() {
        let err = SqlxError::PoolTimedOut;
        let kind = ErrorKind::Database(err);
        pretty_assertions::assert_eq!(kind.to_string(), "Database error");
        assert!(kind.is_internal());
    }

    #[test]
    fn io_error_is_internal() {
        let err = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
        let kind = ErrorKind::Io(err);
        pretty_assertions::assert_eq!(kind.to_string(), "IO error");
        assert!(kind.is_internal());
    }

    #[test]
    fn subscription_validation_display() {
        let kind = ErrorKind::SubscriptionValidation("missing".into());
        pretty_assertions::assert_eq!(kind.to_string(), "missing");
        assert!(!kind.is_internal());
    }

    #[test]
    fn query_validation_display() {
        let kind = ErrorKind::QueryValidation("bad geo".into());
        pretty_assertions::assert_eq!(kind.to_string(), "bad geo");
        assert!(!kind.is_internal());
    }

    #[test]
    fn malformed_request_display() {
        let kind = ErrorKind::MalformedRequest("bad json".into());
        pretty_assertions::assert_eq!(kind.to_string(), "bad json");
        assert!(!kind.is_internal());
    }

    // -- From<SqlxError> mapping ----------------------------------------------

    #[test]
    fn sqlx_unique_violation_maps_to_database_unique_violation() {
        let err = SqlxError::Database(Box::new(FakeDbError::unique_violation()));
        let andromeda_err = AndromedaError::from(err);
        assert!(matches!(andromeda_err.kind(), ErrorKind::DatabaseUniqueViolation(_)));
    }

    #[test]
    fn sqlx_invalid_regex_maps_to_database_regex_error() {
        let err = SqlxError::Database(Box::new(FakeDbError::invalid_regex()));
        let andromeda_err = AndromedaError::from(err);
        assert!(matches!(andromeda_err.kind(), ErrorKind::DatabaseRegexError(_)));
    }

    #[test]
    fn sqlx_other_maps_to_database() {
        let err = SqlxError::PoolTimedOut;
        let andromeda_err = AndromedaError::from(err);
        assert!(matches!(andromeda_err.kind(), ErrorKind::Database(_)));
    }

    // -- From<JsonLdError> mapping --------------------------------------------

    #[test]
    fn jsonld_http_error_maps_to_context_not_available() {
        let err = JsonLdError::ContextHttp {
            url: "https://example.com/context.jsonld".to_string(),
            status: reqwest::StatusCode::NOT_FOUND,
        };
        let andromeda_err = AndromedaError::from(err);
        assert!(matches!(andromeda_err.kind(), ErrorKind::ContextNotAvailable(_)));
    }

    #[test]
    fn jsonld_fetch_disabled_maps_to_context_not_available() {
        let err = JsonLdError::ContextFetchDisabled {
            url: "https://example.com/context.jsonld".to_string(),
        };
        let andromeda_err = AndromedaError::from(err);
        assert!(matches!(andromeda_err.kind(), ErrorKind::ContextNotAvailable(_)));
    }

    #[test]
    fn jsonld_context_load_maps_to_context_not_available() {
        let err = JsonLdError::ContextLoad {
            url: "https://example.com/context.jsonld".to_string(),
            message: "connection refused".to_string(),
        };
        let andromeda_err = AndromedaError::from(err);
        assert!(matches!(andromeda_err.kind(), ErrorKind::ContextNotAvailable(_)));
    }

    #[test]
    fn jsonld_context_too_large_maps_to_context_not_available() {
        let err = JsonLdError::ContextTooLarge {
            url: "https://example.com/context.jsonld".to_string(),
            size: 2_000_000,
            max: 1_000_000,
        };
        let andromeda_err = AndromedaError::from(err);
        assert!(matches!(andromeda_err.kind(), ErrorKind::ContextNotAvailable(_)));
    }

    #[test]
    fn jsonld_context_invalid_maps_to_context_not_available() {
        let err = JsonLdError::ContextInvalid {
            url: "https://example.com/context.jsonld".to_string(),
            message: "invalid JSON-LD".to_string(),
        };
        let andromeda_err = AndromedaError::from(err);
        assert!(matches!(andromeda_err.kind(), ErrorKind::ContextNotAvailable(_)));
    }

    #[test]
    fn jsonld_embedded_context_invalid_maps_to_jsonld_processing() {
        let err = JsonLdError::EmbeddedContextInvalid("bad context".to_string());
        let andromeda_err = AndromedaError::from(err);
        assert!(matches!(andromeda_err.kind(), ErrorKind::JsonLdProcessing(_)));
    }

    #[test]
    fn jsonld_invalid_iri_maps_to_jsonld_processing() {
        let err = JsonLdError::invalid_iri("broken");
        let andromeda_err = AndromedaError::from(err);
        assert!(matches!(andromeda_err.kind(), ErrorKind::JsonLdProcessing(_)));
    }

    // -- From<QueryError> mapping ---------------------------------------------

    #[test]
    fn query_error_data_variant_maps_to_query_validation() {
        let err = QueryError::ParamIdInvalid("bad".to_string());
        let andromeda_err = AndromedaError::from(err);
        assert!(matches!(andromeda_err.kind(), ErrorKind::QueryValidation(_)));
    }

    #[test]
    fn query_error_malformed_variant_maps_to_malformed_request() {
        let err = QueryError::TypeExprEmpty;
        let andromeda_err = AndromedaError::from(err);
        assert!(matches!(andromeda_err.kind(), ErrorKind::MalformedRequest(_)));
    }

    #[test]
    fn query_error_geo_distance_preserves_source_chain() {
        let err = QueryError::GeoInvalidDistance {
            value: "nope".to_string(),
            source: "nope".parse::<f64>().unwrap_err(),
        };
        // err.source() must expose the underlying ParseFloatError for callers
        // that want to introspect the cause.
        let source = std::error::Error::source(&err);
        assert!(source.is_some());
    }

    #[test]
    fn query_error_id_pattern_regex_preserves_source_chain() {
        let err = QueryError::from(regex::Regex::new("[").unwrap_err());
        let source = std::error::Error::source(&err);
        assert!(source.is_some());
    }

    // -- AndromedaError struct tests ------------------------------------------

    #[test]
    fn instance_defaults_to_none() {
        let err = AndromedaError::entity_not_found("x");
        assert!(err.instance().is_none());
    }

    #[test]
    fn with_instance_sets_uri() {
        let uri = Uri::from_static("/test");
        let err = AndromedaError::entity_not_found("x").with_instance(uri);
        pretty_assertions::assert_eq!(err.instance().map(|u| u.to_string()), Some("/test".to_string()));
    }

    #[test]
    fn is_not_found_true_for_not_found_variants() {
        assert!(AndromedaError::entity_not_found("x").is_not_found());
        assert!(AndromedaError::subscription_not_found("x").is_not_found());
        assert!(AndromedaError::csr_not_found("x").is_not_found());
        assert!(AndromedaError::snapshot_not_found("x").is_not_found());
        assert!(AndromedaError::context_not_found("x").is_not_found());
        assert!(AndromedaError::entity_map_not_found("x").is_not_found());
        assert!(AndromedaError::temporal_not_found("x").is_not_found());
    }

    #[test]
    fn is_not_found_false_for_other_variants() {
        assert!(!AndromedaError::entity_already_exists("x").is_not_found());
        assert!(!AndromedaError::internal("x").is_not_found());
    }

    #[test]
    fn display_delegates_to_kind() {
        let err = AndromedaError::entity_not_found("urn:x");
        pretty_assertions::assert_eq!(err.to_string(), "Entity urn:x not found");
    }

    // -- Test helpers ---------------------------------------------------------

    #[derive(Debug)]
    struct FakeDbError {
        message: String,
        code: Option<Cow<'static, str>>,
        kind: SqlxErrorKind,
    }

    impl FakeDbError {
        fn unique_violation() -> FakeDbError {
            FakeDbError {
                message: "duplicate key".to_string(),
                code: Some(Cow::Borrowed("23505")),
                kind: SqlxErrorKind::UniqueViolation,
            }
        }

        fn invalid_regex() -> FakeDbError {
            FakeDbError {
                message: "invalid regular expression: quantifier operand invalid".to_string(),
                code: Some(Cow::Borrowed("2201B")),
                kind: SqlxErrorKind::Other,
            }
        }
    }

    impl fmt::Display for FakeDbError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}", self.message)
        }
    }

    impl StdError for FakeDbError {}

    impl DatabaseError for FakeDbError {
        fn message(&self) -> &str {
            &self.message
        }

        fn code(&self) -> Option<Cow<'_, str>> {
            self.code.clone()
        }

        fn as_error(&self) -> &(dyn StdError + Send + Sync + 'static) {
            self
        }

        fn as_error_mut(&mut self) -> &mut (dyn StdError + Send + Sync + 'static) {
            self
        }

        fn into_error(self: Box<Self>) -> Box<dyn StdError + Send + Sync + 'static> {
            self
        }

        fn kind(&self) -> SqlxErrorKind {
            match self.kind {
                SqlxErrorKind::UniqueViolation => SqlxErrorKind::UniqueViolation,
                SqlxErrorKind::ForeignKeyViolation => SqlxErrorKind::ForeignKeyViolation,
                SqlxErrorKind::NotNullViolation => SqlxErrorKind::NotNullViolation,
                SqlxErrorKind::CheckViolation => SqlxErrorKind::CheckViolation,
                SqlxErrorKind::Other => SqlxErrorKind::Other,
                _ => SqlxErrorKind::Other,
            }
        }
    }
}
