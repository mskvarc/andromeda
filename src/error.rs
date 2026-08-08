//! Domain-centric broker error types.
//!
//! [`ErrorKind`] is a `thiserror` enum whose variants describe *what went wrong*
//! in domain terms (e.g. `EntityNotFound`, `SubscriptionValidation`).  The HTTP
//! layer decides the NGSI-LD error category and status code.
//!
//! [`AndromedaError`] wraps an [`ErrorKind`] with an optional request URI
//! (`instance`) that handlers attach for RFC 7807 Problem Details responses.

use andromeda_jsonld::error::JsonLdError;
use andromeda_ngsi_ld_lexicon::ngsi_ld_urn::{AttributeListIdParseError, EntityMapIdParseError, EntityTypeListIdParseError, SnapshotIdParseError};
use andromeda_ngsi_ld_query::error::QueryError;
use andromeda_ngsi_ld_render::error::RenderError;
use andromeda_ngsi_ld_request::error::ParameterError;
use andromeda_postgresql_context_store::error::ContextStoreError;
use andromeda_postgresql_query::{attribute_name_lookup::AttributeNameLookupError, error::RenderError as StatementRenderError};
use andromeda_postgresql_temporal_store::error::TemporalStoreError;
use andromeda_tenancy::error::TenancyError;
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

    // -- Unsupported operation -----------------------------------------------
    /// The requested operation is not supported by this broker.
    #[error("{0}")]
    OperationNotSupported(String),

    /// The request conflicts with exclusive context-source ownership.
    #[error("{0}")]
    Conflict(String),

    /// A required context-source forwarding operation failed at the gateway.
    #[error("{0}")]
    ContextSourceGateway(String),

    // -- JSON-LD context availability ----------------------------------------
    /// A required JSON-LD context could not be fetched or loaded.
    #[error("{0}")]
    ContextNotAvailable(String),

    // -- Tenancy -------------------------------------------------------------
    /// Reaching or building a tenant's runtime failed.
    ///
    /// Carried typed rather than flattened into a message, for the same reason
    /// as the JSON-LD variant below: clause 5.5.2 reports an unknown tenant as
    /// `NonexistentTenant` and a single-tenant deployment as
    /// `NoMultiTenantSupport`, and which of those a failure is is a variant
    /// rather than a substring.
    #[error(transparent)]
    Tenancy(#[from] TenancyError),

    // -- JSON-LD processing --------------------------------------------------
    /// Resolving a context, or expanding or compacting a document against one,
    /// failed.
    ///
    /// Carried typed rather than flattened into a message: clause 5.5.2 reports
    /// a context that could not be fetched as `LdContextNotAvailable` and a
    /// document that could not be reshaped as `BadRequestData`, and which of
    /// the two a failure is is a predicate on the error rather than a substring
    /// of its rendering.
    #[error(transparent)]
    JsonLd(#[from] JsonLdError),

    /// JSON-LD expansion, compaction, IRI validation, or document parsing
    /// failed, as reported by the broker rather than by the processor.
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
    InvalidUrn(#[source] urn_rs::Error),

    /// A snapshot identifier has invalid NGSI-LD URN format.
    #[error("Invalid snapshot id: {0}")]
    InvalidSnapshotId(#[source] SnapshotIdParseError),

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

    /// A response representation could not be built.
    #[error("Response rendering failed")]
    Rendering(#[from] RenderError),

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
                | ErrorKind::Rendering(_)
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
    pub fn with_instance(mut self, instance: &Uri) -> AndromedaError {
        self.instance = Some(instance.clone());
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

    // -- Request structure ---------------------------------------------------

    /// The request is structurally malformed.
    pub fn malformed_request(message: impl Into<String>) -> AndromedaError {
        ErrorKind::MalformedRequest(message.into()).into()
    }

    /// The request is well formed but this broker's configuration cannot serve
    /// it (clause 5.5.2 `OperationNotSupported`).
    pub fn operation_not_supported(message: impl Into<String>) -> AndromedaError {
        ErrorKind::OperationNotSupported(message.into()).into()
    }

    /// Exclusive context-source ownership conflicts with the requested operation.
    pub fn conflict(message: impl Into<String>) -> AndromedaError {
        ErrorKind::Conflict(message.into()).into()
    }

    /// A mandatory context-source forwarding operation failed.
    pub fn context_source_gateway(message: impl Into<String>) -> AndromedaError {
        ErrorKind::ContextSourceGateway(message.into()).into()
    }

    // -- Infrastructure ------------------------------------------------------

    /// An internal error that does not fit another category.
    pub fn internal(message: impl Into<String>) -> AndromedaError {
        ErrorKind::Internal(message.into()).into()
    }

    /// A temporal store operation failed.
    pub fn temporal_store(error: TemporalStoreError) -> AndromedaError {
        ErrorKind::Internal(error.to_string()).into()
    }
}

/// Extension methods for attaching HTTP request URI context to fallible results.
pub trait ResultInstanceExt<T, E> {
    /// Converts the error into [`AndromedaError`] and attaches `instance`.
    ///
    /// Takes the URI by reference so handlers avoid eagerly cloning on every
    /// request; the clone only happens when the error branch runs.
    fn with_instance(self, instance: &Uri) -> Result<T, AndromedaError>;
}

impl<T, E> ResultInstanceExt<T, E> for Result<T, E>
where
    E: Into<AndromedaError>,
{
    fn with_instance(self, instance: &Uri) -> Result<T, AndromedaError> {
        self.map_err(|error| error.into().with_instance(instance))
    }
}

// -- From<ErrorKind> for AndromedaError --------------------------------------

impl From<ErrorKind> for AndromedaError {
    fn from(kind: ErrorKind) -> AndromedaError {
        AndromedaError { kind, instance: None }
    }
}

/// An EWKB codec failure is never client-caused: the geometry was already
/// validated as GeoJSON before it reached the store boundary, so a failure here
/// is a broker or driver fault.
mod conversion;
#[cfg(test)]
mod tests {
    // A test deliberately feeds a malformed pattern to the `Database` mapping;
    // `invalid_regex` is a correctness lint that fires on the literal itself.
    #![allow(clippy::invalid_regex)]
    use super::*;
    use andromeda_ngsi_ld_lexicon::tenant::TenantName;
    use iri_rs::IriBuf;
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
    fn a_tenancy_failure_reaches_this_layer_typed_rather_than_as_a_message() {
        let kind = ErrorKind::Tenancy(TenancyError::UnknownTenant {
            name: TenantName::try_from("t1".to_string()).expect("a valid tenant name"),
        });
        pretty_assertions::assert_eq!(kind.to_string(), "tenant 't1' does not exist");
        assert!(!kind.is_internal());
    }

    #[test]
    fn a_single_tenant_deployment_reports_the_refusal_as_its_own_variant() {
        let kind = ErrorKind::Tenancy(TenancyError::MultiTenancyDisabled);
        pretty_assertions::assert_eq!(kind.to_string(), "multi-tenant support is not enabled");
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
    fn a_jsonld_failure_reaches_this_layer_typed_rather_than_as_a_message() {
        // Each of these used to be flattened into a formatted string, which
        // meant the clause 5.5.2 category had to be re-derived from the
        // variant here instead of being asked of the error itself.
        let iri = || IriBuf::new("https://example.com/context.jsonld".to_string()).expect("valid IRI");
        let failures = [
            JsonLdError::ContextHttp {
                url: iri(),
                status: reqwest::StatusCode::NOT_FOUND,
            },
            JsonLdError::ContextFetchDisabled { url: iri() },
            JsonLdError::ContextLoad {
                url: iri(),
                message: "connection refused".to_string(),
            },
            JsonLdError::ContextTooLarge {
                url: iri(),
                size: 2_000_000,
                max: 1_000_000,
            },
            JsonLdError::ContextInvalid {
                url: iri(),
                message: "invalid JSON-LD".to_string(),
            },
            JsonLdError::EmbeddedContextInvalid("bad context".to_string()),
            JsonLdError::invalid_iri("broken"),
        ];

        for failure in failures {
            let rendered = failure.to_string();
            let error = AndromedaError::from(failure);
            match error.kind() {
                ErrorKind::JsonLd(carried) => pretty_assertions::assert_eq!(carried.to_string(), rendered),
                other => unreachable!("expected ErrorKind::JsonLd, got {other:?}"),
            }
        }
    }

    #[test]
    fn a_context_loading_failure_stays_distinguishable_from_a_document_failure() {
        let unavailable = AndromedaError::from(JsonLdError::ContextFetchDisabled {
            url: IriBuf::new("https://example.com/context.jsonld".to_string()).expect("valid IRI"),
        });
        let malformed = AndromedaError::from(JsonLdError::invalid_document("not an object"));

        match (unavailable.kind(), malformed.kind()) {
            (ErrorKind::JsonLd(unavailable), ErrorKind::JsonLd(malformed)) => {
                assert!(unavailable.is_context_not_available());
                assert!(!malformed.is_context_not_available());
            }
            (first, second) => unreachable!("expected two ErrorKind::JsonLd, got {first:?} and {second:?}"),
        }
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
        let err = QueryError::GeoRel(andromeda_ngsi_ld_geometry::geo_rel::GeoRelParseError::InvalidDistance {
            value: "nope".to_string(),
            source: "nope".parse::<f64>().unwrap_err(),
        });
        // err.source() must expose the underlying georel parse error for
        // callers that want to introspect the cause.
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
        let err = AndromedaError::entity_not_found("x").with_instance(&uri);
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
