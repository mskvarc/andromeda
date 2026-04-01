use axum::http::Uri;
use std::{error::Error as StdError, fmt};

use confique::Error as ConfiqueError;
use sqlx::{Error as SqlxError, migrate::MigrateError};
use tracing::subscriber::SetGlobalDefaultError;
use urn::Error as UrnError;

use crate::{error::ngsi_ld::NgsiLdErrorType, jsonld::Error as JsonLdError, query::error::QueryError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AndromedaErrorKind {
    InvalidRequest,
    BadRequestData,
    NotFound,
    AlreadyExists,
    #[allow(dead_code)]
    Conflict,
    LdContextNotAvailable,
    NonexistentTenant,
    NoMultiTenantSupport,
    #[allow(dead_code)]
    OperationNotSupported,
    Repository,
    ExternalDependency,
    Internal,
}

impl AndromedaErrorKind {
    /// Maps this error kind to its corresponding NGSI-LD error type.
    pub fn to_ngsi_ld_error_type(self) -> NgsiLdErrorType {
        match self {
            AndromedaErrorKind::InvalidRequest => NgsiLdErrorType::InvalidRequest,
            AndromedaErrorKind::BadRequestData => NgsiLdErrorType::BadRequestData,
            AndromedaErrorKind::NotFound => NgsiLdErrorType::ResourceNotFound,
            AndromedaErrorKind::AlreadyExists => NgsiLdErrorType::AlreadyExists,
            AndromedaErrorKind::Conflict => NgsiLdErrorType::Conflict,
            AndromedaErrorKind::LdContextNotAvailable => NgsiLdErrorType::LdContextNotAvailable,
            AndromedaErrorKind::NoMultiTenantSupport => NgsiLdErrorType::NoMultiTenantSupport,
            AndromedaErrorKind::NonexistentTenant => NgsiLdErrorType::NonexistentTenant,
            AndromedaErrorKind::OperationNotSupported => NgsiLdErrorType::OperationNotSupported,
            AndromedaErrorKind::Repository | AndromedaErrorKind::ExternalDependency | AndromedaErrorKind::Internal => NgsiLdErrorType::InternalError,
        }
    }

    /// Returns `true` if this error kind represents an internal/infrastructure failure
    /// whose details should be sanitized before sending to clients.
    pub fn is_internal(self) -> bool {
        matches!(
            self,
            AndromedaErrorKind::Repository | AndromedaErrorKind::ExternalDependency | AndromedaErrorKind::Internal
        )
    }
}

#[derive(Debug, Default, Clone)]
pub struct ErrorContext {
    pub instance: Option<Uri>,
    pub entity_id: Option<String>,
    #[allow(unused)]
    pub query: Option<String>,
    pub op: Option<&'static str>,
}

#[derive(Debug)]
pub struct AndromedaError {
    inner: Box<AndromedaErrorInner>,
}

#[derive(Debug)]
struct AndromedaErrorInner {
    kind: AndromedaErrorKind,
    message: String,
    source: Option<Box<dyn StdError + Send + Sync>>,
    context: ErrorContext,
    retryable: bool,
}

impl AndromedaError {
    pub fn new(kind: AndromedaErrorKind, message: impl Into<String>) -> AndromedaError {
        AndromedaError {
            inner: Box::new(AndromedaErrorInner {
                kind,
                message: message.into(),
                source: None,
                context: ErrorContext::default(),
                retryable: false,
            }),
        }
    }

    pub fn kind(&self) -> AndromedaErrorKind {
        self.inner.kind
    }

    pub fn message(&self) -> &str {
        &self.inner.message
    }

    pub fn context(&self) -> &ErrorContext {
        &self.inner.context
    }

    #[allow(unused)]
    pub fn retryable(&self) -> bool {
        self.inner.retryable
    }

    pub fn with_source<E>(mut self, source: E) -> AndromedaError
    where
        E: StdError + Send + Sync + 'static,
    {
        self.inner.source = Some(Box::new(source));
        self
    }

    pub fn with_instance(mut self, instance: Uri) -> AndromedaError {
        self.inner.context.instance = Some(instance);
        self
    }

    pub fn with_entity_id(mut self, entity_id: impl Into<String>) -> AndromedaError {
        self.inner.context.entity_id = Some(entity_id.into());
        self
    }

    #[allow(unused)]
    pub fn with_query(mut self, query: impl Into<String>) -> AndromedaError {
        self.inner.context.query = Some(query.into());
        self
    }

    pub fn with_op(mut self, op: &'static str) -> AndromedaError {
        self.inner.context.op = Some(op);
        self
    }

    pub fn with_retryable(mut self, retryable: bool) -> AndromedaError {
        self.inner.retryable = retryable;
        self
    }

    // -- Factory methods --

    pub fn invalid_request(message: impl Into<String>) -> AndromedaError {
        AndromedaError::new(AndromedaErrorKind::InvalidRequest, message)
    }

    pub fn bad_request_data(message: impl Into<String>) -> AndromedaError {
        AndromedaError::new(AndromedaErrorKind::BadRequestData, message)
    }

    pub fn not_found(entity_id: impl Into<String>) -> AndromedaError {
        let entity_id = entity_id.into();
        AndromedaError::new(AndromedaErrorKind::NotFound, format!("Entity {} not found", entity_id)).with_entity_id(entity_id)
    }

    pub fn already_exists(entity_id: impl Into<String>) -> AndromedaError {
        let entity_id = entity_id.into();
        AndromedaError::new(AndromedaErrorKind::AlreadyExists, format!("Entity {} already exists", entity_id)).with_entity_id(entity_id)
    }

    #[allow(dead_code)]
    pub fn conflict(message: impl Into<String>) -> AndromedaError {
        AndromedaError::new(AndromedaErrorKind::Conflict, message)
    }

    pub fn ld_context_not_available(message: impl Into<String>) -> AndromedaError {
        AndromedaError::new(AndromedaErrorKind::LdContextNotAvailable, message)
    }

    pub fn nonexistent_tenant(message: impl Into<String>) -> AndromedaError {
        AndromedaError::new(AndromedaErrorKind::NonexistentTenant, message)
    }

    pub fn no_multi_tenant_support(message: impl Into<String>) -> AndromedaError {
        AndromedaError::new(AndromedaErrorKind::NoMultiTenantSupport, message)
    }

    #[allow(dead_code)]
    pub fn operation_not_supported(message: impl Into<String>) -> AndromedaError {
        AndromedaError::new(AndromedaErrorKind::OperationNotSupported, message)
    }

    pub fn repository(message: impl Into<String>) -> AndromedaError {
        AndromedaError::new(AndromedaErrorKind::Repository, message)
    }

    pub fn external_dependency(message: impl Into<String>) -> AndromedaError {
        AndromedaError::new(AndromedaErrorKind::ExternalDependency, message)
    }

    pub fn internal(message: impl Into<String>) -> AndromedaError {
        AndromedaError::new(AndromedaErrorKind::Internal, message)
    }

    // -- Explicit adapter functions --

    /// Maps a sqlx database error to an `AndromedaError` with operation context.
    ///
    /// Handles well-known PostgreSQL error codes:
    /// - `23505` (unique violation) → `AlreadyExists`
    /// - `2201B` (invalid regex) → `BadRequestData`
    /// - All others → `Repository`
    pub fn map_sqlx(error: SqlxError, op: &'static str) -> AndromedaError {
        if let SqlxError::Database(ref db_error) = error {
            if db_error.code().as_deref() == Some("23505") {
                return AndromedaError::already_exists("Resource already exists").with_source(error).with_op(op);
            }
            if db_error.code().as_deref() == Some("2201B") {
                return AndromedaError::bad_request_data("Invalid regular expression").with_source(error).with_op(op);
            }
        }

        AndromedaError::repository("Database error").with_source(error).with_op(op)
    }

    /// Maps a JSON-LD processing error to an `AndromedaError` with operation context.
    pub fn map_jsonld(error: JsonLdError, op: &'static str) -> AndromedaError {
        let err = match error {
            JsonLdError::ContextFetchDisabled { ref url } => AndromedaError::ld_context_not_available(format!("Context fetch disabled for {}", url)),
            JsonLdError::ContextHttp { ref url, status } => AndromedaError::ld_context_not_available(format!("HTTP {} while loading context {}", status, url)),
            JsonLdError::ContextLoad { ref url, ref message } => {
                AndromedaError::ld_context_not_available(format!("Failed to load context {}: {}", url, message))
            }
            JsonLdError::ContextTooLarge { ref url, size, max } => {
                AndromedaError::ld_context_not_available(format!("Context {} too large ({} > {} bytes)", url, size, max))
            }
            JsonLdError::ContextInvalid { ref url, ref message } => AndromedaError::ld_context_not_available(format!("Invalid context {}: {}", url, message)),
            JsonLdError::EmbeddedContextInvalid(ref message) => AndromedaError::bad_request_data(format!("Embedded context invalid: {}", message)),
            JsonLdError::Expansion(_) | JsonLdError::Compaction(_) => AndromedaError::bad_request_data(error.to_string()).with_source(error),
            JsonLdError::InvalidIri(_) | JsonLdError::InvalidDocument(_) | JsonLdError::Json(_) => AndromedaError::bad_request_data(error.to_string()),
        };
        err.with_op(op)
    }

    /// Maps a reqwest HTTP client error to an `AndromedaError` with operation context.
    ///
    /// Sets `retryable = true` for timeout and connection errors.
    pub fn map_reqwest(error: reqwest::Error, op: &'static str) -> AndromedaError {
        let retryable = error.is_timeout() || error.is_connect();
        let message = format!("External request failed: {}", error);
        AndromedaError::external_dependency(message)
            .with_source(error)
            .with_op(op)
            .with_retryable(retryable)
    }
}

impl fmt::Display for AndromedaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.inner.message)
    }
}

impl StdError for AndromedaError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.inner.source.as_deref().map(|err| err as &(dyn StdError + 'static))
    }
}

impl From<ConfiqueError> for AndromedaError {
    fn from(error: ConfiqueError) -> AndromedaError {
        AndromedaError::internal("Loading configuration failed").with_source(error)
    }
}

impl From<SetGlobalDefaultError> for AndromedaError {
    fn from(error: SetGlobalDefaultError) -> AndromedaError {
        AndromedaError::internal("Failed to set default tracing subscriber").with_source(error)
    }
}

impl From<std::io::Error> for AndromedaError {
    fn from(error: std::io::Error) -> AndromedaError {
        AndromedaError::internal("IO error").with_source(error)
    }
}

impl From<UrnError> for AndromedaError {
    fn from(error: UrnError) -> AndromedaError {
        AndromedaError::invalid_request("Invalid URN").with_source(error)
    }
}

impl From<serde_json::Error> for AndromedaError {
    fn from(error: serde_json::Error) -> AndromedaError {
        AndromedaError::bad_request_data("JSON error").with_source(error)
    }
}

impl From<MigrateError> for AndromedaError {
    fn from(error: MigrateError) -> AndromedaError {
        AndromedaError::repository("Database migration error").with_source(error)
    }
}

impl From<SqlxError> for AndromedaError {
    fn from(error: SqlxError) -> AndromedaError {
        AndromedaError::map_sqlx(error, "unknown")
    }
}

impl From<JsonLdError> for AndromedaError {
    fn from(error: JsonLdError) -> AndromedaError {
        AndromedaError::map_jsonld(error, "unknown")
    }
}

impl From<QueryError> for AndromedaError {
    fn from(err: QueryError) -> AndromedaError {
        match err {
            QueryError::BadRequestData(msg) => AndromedaError::bad_request_data(msg),
            QueryError::InvalidRequest(msg) => AndromedaError::invalid_request(msg),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AndromedaError, AndromedaErrorKind};
    use crate::jsonld::Error as JsonLdError;
    use sqlx::{
        Error as SqlxError,
        error::{DatabaseError, ErrorKind},
    };
    use std::{borrow::Cow, error::Error as StdError, fmt};

    #[test]
    fn jsonld_http_error_maps_to_ld_context_not_available() {
        let err = JsonLdError::ContextHttp {
            url: "https://example.com/context.jsonld".to_string(),
            status: reqwest::StatusCode::NOT_FOUND,
        };

        let mapped = AndromedaError::map_jsonld(err, "test_expand");
        assert_eq!(mapped.kind(), AndromedaErrorKind::LdContextNotAvailable);
        assert_eq!(mapped.context().op, Some("test_expand"));
    }

    #[test]
    fn jsonld_invalid_iri_maps_to_bad_request_data() {
        let err = JsonLdError::invalid_iri("broken");
        let mapped = AndromedaError::map_jsonld(err, "test_expand");
        assert_eq!(mapped.kind(), AndromedaErrorKind::BadRequestData);
    }

    #[test]
    fn jsonld_context_invalid_maps_to_ld_context_not_available() {
        let err = JsonLdError::ContextInvalid {
            url: "https://example.com/context.jsonld".to_string(),
            message: "invalid JSON-LD".to_string(),
        };
        let mapped = AndromedaError::map_jsonld(err, "test_expand");
        assert_eq!(mapped.kind(), AndromedaErrorKind::LdContextNotAvailable);
    }

    #[test]
    fn sqlx_unique_violation_maps_to_already_exists() {
        let err = SqlxError::Database(Box::new(FakeDbError::unique_violation()));
        let mapped = AndromedaError::map_sqlx(err, "entity_create");
        assert_eq!(mapped.kind(), AndromedaErrorKind::AlreadyExists);
        assert_eq!(mapped.context().op, Some("entity_create"));
    }

    #[test]
    fn sqlx_invalid_regex_maps_to_bad_request_data() {
        let err = SqlxError::Database(Box::new(FakeDbError::invalid_regex()));
        let mapped = AndromedaError::map_sqlx(err, "entity_query");
        assert_eq!(mapped.kind(), AndromedaErrorKind::BadRequestData);
    }

    #[test]
    fn retryable_defaults_to_false() {
        let err = AndromedaError::internal("test");
        assert!(!err.retryable());
    }

    #[test]
    fn with_retryable_sets_flag() {
        let err = AndromedaError::external_dependency("timeout").with_retryable(true);
        assert!(err.retryable());
        assert_eq!(err.kind(), AndromedaErrorKind::ExternalDependency);
    }

    #[test]
    fn with_op_sets_operation_context() {
        let err = AndromedaError::internal("test").with_op("snapshot_create");
        assert_eq!(err.context().op, Some("snapshot_create"));
    }

    #[derive(Debug)]
    struct FakeDbError {
        message: String,
        code: Option<Cow<'static, str>>,
        kind: ErrorKind,
    }

    impl FakeDbError {
        fn unique_violation() -> FakeDbError {
            FakeDbError {
                message: "duplicate key".to_string(),
                code: Some(Cow::Borrowed("23505")),
                kind: ErrorKind::UniqueViolation,
            }
        }

        fn invalid_regex() -> FakeDbError {
            FakeDbError {
                message: "invalid regular expression: quantifier operand invalid".to_string(),
                code: Some(Cow::Borrowed("2201B")),
                kind: ErrorKind::Other,
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

        fn kind(&self) -> ErrorKind {
            match self.kind {
                ErrorKind::UniqueViolation => ErrorKind::UniqueViolation,
                ErrorKind::ForeignKeyViolation => ErrorKind::ForeignKeyViolation,
                ErrorKind::NotNullViolation => ErrorKind::NotNullViolation,
                ErrorKind::CheckViolation => ErrorKind::CheckViolation,
                ErrorKind::Other => ErrorKind::Other,
                _ => ErrorKind::Other,
            }
        }
    }
}
