//! Focused failures produced while loading, validating, or rendering configuration.

use crate::config::{management::ManagementConfigError, notifications::NotificationsConfigError};
use andromeda_entity::{config::EntityConfigError, spatial_config::SpatialConfigError};
use andromeda_extensions::config::ExtensionsConfigError;
use andromeda_federation::config::FederationConfigError;
use andromeda_http_server::config::ServerConfigError;
use andromeda_http_stack::config::HttpStackConfigError;
use andromeda_jsonld::config::JsonLdConfigError;
use andromeda_ngsi_ld_request::pagination::policy::PaginationPolicyError;
use andromeda_postgresql_context_store::config::DatabaseConfigError;
use andromeda_server_tls::config::TlsConfigError;
use andromeda_snapshot::config::SnapshotConfigError;
use andromeda_temporal::config::TemporalConfigError;
use andromeda_tenancy::config::TenancyConfigError;
use figment2::Error as FigmentError;
use humantime::format_duration;
use serde_json::Error as JsonError;
use std::{path::PathBuf, time::Duration};
use thiserror::Error;
use toml::ser::Error as TomlError;
use url::ParseError;

/// Failure owned by the broker configuration boundary.
#[derive(Debug, Error)]
pub(crate) enum ConfigurationError {
    /// Layered configuration loading failed.
    ///
    /// The figment error is boxed: it is by far the largest variant, and an
    /// unboxed copy would make every `Result<_, ConfigurationError>` in the
    /// loader carry its full width.
    #[error("loading configuration failed: {0}")]
    Load(#[source] Box<FigmentError>),
    /// An explicitly requested configuration file does not exist.
    ///
    /// A `--config` path names a file the operator expects to be read, so a
    /// missing one is a hard error; the default configuration file, by
    /// contrast, is loaded only when present.
    #[error("configuration file not found: {0}")]
    MissingFile(PathBuf),
    /// The `[server]` section violated its declared invariant.
    #[error("invalid configuration: {0}")]
    Server(#[from] ServerConfigError),
    /// The `[http]` section violated its declared invariant.
    #[error("invalid configuration: {0}")]
    Http(#[from] HttpStackConfigError),
    /// The `[database]` section violated its declared invariant.
    #[error("invalid configuration: {0}")]
    Database(#[from] DatabaseConfigError),
    /// The `[jsonld]` section violated its declared invariant.
    #[error("invalid configuration: {0}")]
    JsonLd(#[from] JsonLdConfigError),
    /// The `[temporal]` section violated its declared invariant.
    #[error("invalid configuration: {0}")]
    Temporal(#[from] TemporalConfigError),
    /// The `[tenancy]` section violated its declared invariant.
    #[error("invalid configuration: {0}")]
    Tenancy(#[from] TenancyConfigError),
    /// The `[entity]` section violated its declared invariant.
    #[error("invalid configuration: {0}")]
    Entity(#[from] EntityConfigError),
    /// The `[federation]` section violated its declared invariant.
    #[error("invalid configuration: {0}")]
    Federation(#[from] FederationConfigError),
    /// The `[snapshot]` section violated its declared invariant.
    #[error("invalid configuration: {0}")]
    Snapshot(#[from] SnapshotConfigError),
    /// The `[notifications]` section violated its declared invariant.
    #[error("invalid configuration: {0}")]
    Notifications(#[from] NotificationsConfigError),
    /// The `[spatial]` section violated its declared invariant.
    #[error("invalid configuration: {0}")]
    Spatial(#[from] SpatialConfigError),
    /// The `[management]` section violated its declared invariant.
    #[error("invalid configuration: {0}")]
    Management(#[from] ManagementConfigError),
    /// The `[extensions]` section violated its declared invariant.
    #[error("invalid configuration: {0}")]
    Extensions(#[from] ExtensionsConfigError),
    /// `[pagination]` and `[extensions]` together name no pagination policy.
    #[error("invalid configuration: {0}")]
    Pagination(#[from] PaginationPolicyError),
    /// A listener that terminates TLS has no usable `[tls]` identity.
    #[error("invalid configuration: {0}")]
    Tls(#[from] TlsConfigError),
    /// The connection acquire deadline is not below the HTTP request timeout.
    #[error(
        "invalid configuration: database.acquire_timeout ({}) must be below http.request_timeout ({}) so a saturated pool answers 503 before the request deadline",
        format_duration(*acquire),
        format_duration(*request)
    )]
    AcquireTimeoutNotBelowRequestTimeout {
        /// The configured acquire deadline.
        acquire: Duration,
        /// The configured request timeout.
        request: Duration,
    },
    /// The tenant cold-build admission deadline is not below the HTTP request
    /// timeout.
    #[error(
        "invalid configuration: tenancy.build_admission_timeout ({}) must be below http.request_timeout ({}) so a request that cannot be admitted to a cold build answers 503 before the request deadline",
        format_duration(*admission),
        format_duration(*request)
    )]
    BuildAdmissionNotBelowRequestTimeout {
        /// The configured admission deadline.
        admission: Duration,
        /// The configured request timeout.
        request: Duration,
    },
    /// The effective configuration could not be rendered as JSON.
    #[error("failed to serialize effective configuration as JSON: {0}")]
    Json(#[from] JsonError),
    /// The effective configuration could not be rendered as TOML.
    #[error("failed to serialize effective configuration as TOML: {0}")]
    Toml(#[from] TomlError),
    /// The compile-time redaction URL is invalid.
    #[error("invalid built-in redacted database URL: {0}")]
    RedactionUrl(#[from] ParseError),
}
