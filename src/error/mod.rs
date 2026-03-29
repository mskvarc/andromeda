use axum::http::Uri;
use std::{error::Error as StdError, fmt};

use confique::Error as ConfiqueError;
use sqlx::{Error as SqlxError, migrate::MigrateError};
use tracing::subscriber::SetGlobalDefaultError;
use urn::Error as UrnError;

use crate::{jsonld::Error as JsonLdError, query::error::QueryError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AndromedaErrorKind {
    InvalidRequest,
    BadRequestData,
    NotFound,
    AlreadyExists,
    Conflict,
    LdContextNotAvailable,
    NonexistentTenant,
    NoMultiTenantSupport,
    #[allow(unused)]
    OperationNotSupported,
    Repository,
    #[allow(unused)]
    Json,
    Io,
    Config,
    Internal,
}

#[derive(Debug, Default, Clone)]
pub struct ErrorContext {
    pub instance: Option<Uri>,
    pub entity_id: Option<String>,
    #[allow(unused)]
    pub query: Option<String>,
}

#[derive(Debug)]
pub struct AndromedaError {
    kind: AndromedaErrorKind,
    message: String,
    source: Option<Box<dyn StdError + Send + Sync>>,
    context: ErrorContext,
}

impl AndromedaError {
    pub fn new(kind: AndromedaErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            source: None,
            context: ErrorContext::default(),
        }
    }

    pub fn kind(&self) -> AndromedaErrorKind {
        self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn context(&self) -> &ErrorContext {
        &self.context
    }

    pub fn with_source<E>(mut self, source: E) -> Self
    where
        E: StdError + Send + Sync + 'static,
    {
        self.source = Some(Box::new(source));
        self
    }

    pub fn with_instance(mut self, instance: Uri) -> Self {
        self.context.instance = Some(instance);
        self
    }

    pub fn with_entity_id(mut self, entity_id: impl Into<String>) -> Self {
        self.context.entity_id = Some(entity_id.into());
        self
    }

    #[allow(unused)]
    pub fn with_query(mut self, query: impl Into<String>) -> Self {
        self.context.query = Some(query.into());
        self
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(AndromedaErrorKind::InvalidRequest, message)
    }

    pub fn bad_request_data(message: impl Into<String>) -> Self {
        Self::new(AndromedaErrorKind::BadRequestData, message)
    }

    pub fn not_found(entity_id: impl Into<String>) -> Self {
        let entity_id = entity_id.into();
        Self::new(AndromedaErrorKind::NotFound, format!("Entity {} not found", entity_id)).with_entity_id(entity_id)
    }

    pub fn already_exists(entity_id: impl Into<String>) -> Self {
        let entity_id = entity_id.into();
        Self::new(AndromedaErrorKind::AlreadyExists, format!("Entity {} already exists", entity_id)).with_entity_id(entity_id)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(AndromedaErrorKind::Conflict, message)
    }

    pub fn ld_context_not_available(message: impl Into<String>) -> Self {
        Self::new(AndromedaErrorKind::LdContextNotAvailable, message)
    }

    pub fn nonexistent_tenant(message: impl Into<String>) -> Self {
        Self::new(AndromedaErrorKind::NonexistentTenant, message)
    }

    pub fn no_multi_tenant_support(message: impl Into<String>) -> Self {
        Self::new(AndromedaErrorKind::NoMultiTenantSupport, message)
    }

    #[allow(unused)]
    pub fn operation_not_supported(message: impl Into<String>) -> Self {
        Self::new(AndromedaErrorKind::OperationNotSupported, message)
    }

    pub fn repository(message: impl Into<String>) -> Self {
        Self::new(AndromedaErrorKind::Repository, message)
    }

    #[allow(unused)]
    pub fn json(message: impl Into<String>) -> Self {
        Self::new(AndromedaErrorKind::Json, message)
    }

    pub fn io(message: impl Into<String>) -> Self {
        Self::new(AndromedaErrorKind::Io, message)
    }

    pub fn config(message: impl Into<String>) -> Self {
        Self::new(AndromedaErrorKind::Config, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(AndromedaErrorKind::Internal, message)
    }
}

impl fmt::Display for AndromedaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl StdError for AndromedaError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.source.as_deref().map(|err| err as &(dyn StdError + 'static))
    }
}

impl From<ConfiqueError> for AndromedaError {
    fn from(error: ConfiqueError) -> Self {
        AndromedaError::config("Loading configuration failed").with_source(error)
    }
}

impl From<SetGlobalDefaultError> for AndromedaError {
    fn from(error: SetGlobalDefaultError) -> Self {
        AndromedaError::internal("Failed to set default tracing subscriber").with_source(error)
    }
}

impl From<std::io::Error> for AndromedaError {
    fn from(error: std::io::Error) -> Self {
        AndromedaError::io("IO error").with_source(error)
    }
}

impl From<UrnError> for AndromedaError {
    fn from(error: UrnError) -> Self {
        AndromedaError::invalid_request("Invalid URN").with_source(error)
    }
}

impl From<serde_json::Error> for AndromedaError {
    fn from(error: serde_json::Error) -> Self {
        AndromedaError::bad_request_data("JSON error").with_source(error)
    }
}

impl From<MigrateError> for AndromedaError {
    fn from(error: MigrateError) -> Self {
        AndromedaError::repository("Database migration error").with_source(error)
    }
}

impl From<SqlxError> for AndromedaError {
    fn from(error: SqlxError) -> Self {
        if let SqlxError::Database(db_error) = &error {
            if db_error.code().as_deref() == Some("23505") {
                return AndromedaError::already_exists("Resource already exists").with_source(error);
            }
            if db_error.code().as_deref() == Some("2201B") {
                return AndromedaError::bad_request_data("Invalid regular expression").with_source(error);
            }
        }

        AndromedaError::repository("Database error").with_source(error)
    }
}

impl From<JsonLdError> for AndromedaError {
    fn from(error: JsonLdError) -> Self {
        match error {
            JsonLdError::ContextFetchDisabled { url } => AndromedaError::ld_context_not_available(format!("Context fetch disabled for {}", url)),
            JsonLdError::ContextHttp { url, status } => AndromedaError::ld_context_not_available(format!("HTTP {} while loading context {}", status, url)),
            JsonLdError::ContextLoad { url, message } => AndromedaError::ld_context_not_available(format!("Failed to load context {}: {}", url, message)),
            JsonLdError::ContextTooLarge { url, size, max } => {
                AndromedaError::ld_context_not_available(format!("Context {} too large ({} > {} bytes)", url, size, max))
            }
            JsonLdError::ContextInvalid { url, message } => AndromedaError::ld_context_not_available(format!("Invalid context {}: {}", url, message)),
            JsonLdError::EmbeddedContextInvalid(message) => AndromedaError::internal(format!("Embedded context invalid: {}", message)),
            JsonLdError::ExpansionError(_) | JsonLdError::CompactionError(_) => AndromedaError::bad_request_data(error.to_string()),
            JsonLdError::InvalidIri(_) | JsonLdError::InvalidDocument(_) | JsonLdError::JsonError(_) => AndromedaError::bad_request_data(error.to_string()),
        }
    }
}

impl From<QueryError> for AndromedaError {
    fn from(err: QueryError) -> Self {
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

        let mapped: AndromedaError = err.into();
        assert_eq!(mapped.kind(), AndromedaErrorKind::LdContextNotAvailable);
    }

    #[test]
    fn jsonld_invalid_iri_maps_to_bad_request_data() {
        let err = JsonLdError::invalid_iri("broken");
        let mapped: AndromedaError = err.into();
        assert_eq!(mapped.kind(), AndromedaErrorKind::BadRequestData);
    }

    #[test]
    fn jsonld_context_invalid_maps_to_ld_context_not_available() {
        let err = JsonLdError::ContextInvalid {
            url: "https://example.com/context.jsonld".to_string(),
            message: "invalid JSON-LD".to_string(),
        };
        let mapped: AndromedaError = err.into();
        assert_eq!(mapped.kind(), AndromedaErrorKind::LdContextNotAvailable);
    }

    #[test]
    fn sqlx_unique_violation_maps_to_already_exists() {
        let err = SqlxError::Database(Box::new(FakeDbError::unique_violation()));
        let mapped: AndromedaError = err.into();
        assert_eq!(mapped.kind(), AndromedaErrorKind::AlreadyExists);
    }

    #[test]
    fn sqlx_invalid_regex_maps_to_bad_request_data() {
        let err = SqlxError::Database(Box::new(FakeDbError::invalid_regex()));
        let mapped: AndromedaError = err.into();
        assert_eq!(mapped.kind(), AndromedaErrorKind::BadRequestData);
    }

    #[derive(Debug)]
    struct FakeDbError {
        message: String,
        code: Option<Cow<'static, str>>,
        kind: ErrorKind,
    }

    impl FakeDbError {
        fn unique_violation() -> Self {
            Self {
                message: "duplicate key".to_string(),
                code: Some(Cow::Borrowed("23505")),
                kind: ErrorKind::UniqueViolation,
            }
        }

        fn invalid_regex() -> Self {
            Self {
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
