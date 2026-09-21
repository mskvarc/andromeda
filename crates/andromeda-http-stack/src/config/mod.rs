//! Configuration for process-wide inbound HTTP mechanics.
//!
//! Each edge feature is a strongly-typed sub-configuration in its own module.
//! Three conventions run through them. An on/off umbrella is an
//! [`HttpFeatureMode`](feature_mode::HttpFeatureMode). An enumerable set of
//! independent choices -- the content codings -- is a typed
//! [`EnumSet`](enumset::EnumSet) listed as a token array, never a family of
//! boolean fields. A genuine policy boolean that maps to a boolean wire semantic
//! (`allow_credentials`, `pass_through_unaccepted`, `allow_private_network`)
//! stays a bare `bool`. Sizes and durations use their human-readable spellings
//! ([`ByteSize`], [`HumanDuration`](human_duration::HumanDuration)).

pub mod compression;
pub mod content_coding;
pub mod cors;
pub mod csrf;
pub mod decompression;
pub mod feature_mode;
pub mod helmet;
pub mod human_duration;
pub mod server_timing;

use crate::config::{
    compression::CompressionConfig,
    cors::CorsConfig,
    csrf::CsrfConfig,
    decompression::DecompressionConfig,
    helmet::HelmetConfig,
    human_duration::HumanDuration,
    server_timing::ServerTimingConfig,
};
use andromeda_http_headers::forwarded_header_trust::ForwardedHeaderTrust;
use bytesize::ByteSize;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use toml_comment::TomlComment;

/// Invalid process-wide HTTP stack configuration.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum HttpStackConfigError {
    /// A request requires a positive timeout.
    #[error("http.request_timeout must be greater than zero")]
    ZeroRequestTimeout,
    /// Request bodies require a positive size ceiling.
    #[error("http.max_request_body must be greater than zero")]
    ZeroRequestBodyLimit,
    /// The Fetch standard forbids `*` with credentials, and the CORS layer
    /// panics on the combination, so it is rejected here.
    #[error("http.cors: allow_credentials cannot be combined with an `any` wildcard on origins, methods, headers or exposed headers")]
    CorsCredentialsWithWildcard,
}

/// Process-wide inbound HTTP stack configuration.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone, PartialEq, Eq)]
pub struct HttpStackConfig {
    /// Maximum wall-clock time for one HTTP request, in human-readable form
    /// (`"30s"`, `"2m"`).
    #[toml_comment(inline)]
    pub request_timeout: HumanDuration,
    /// Maximum accepted HTTP request body size, in human-readable form
    /// (`"64 MiB"`).
    #[toml_comment(inline)]
    pub max_request_body: ByteSize,
    /// Whether the `Forwarded` and `X-Forwarded-Proto` fields of an inbound
    /// request may decide the scheme of the URLs the broker mints from the
    /// request origin.
    ///
    /// Untrusted by default. Both fields are ordinary request fields, so
    /// believing them without a reverse proxy that overwrites them lets any
    /// client choose the scheme of a `Location` field, a pagination `Link`, and
    /// the durably stored `@context` URL of clause 5.13. Turn it on only
    /// together with the proxy configuration `docs/tls-deployment.md`
    /// describes.
    #[toml_comment(inline)]
    pub forwarded_headers: ForwardedHeaderTrust,
    /// CORS policy for browser-facing clients.
    pub cors: CorsConfig,
    /// Response-compression policy.
    pub compression: CompressionConfig,
    /// Request-decompression policy.
    pub decompression: DecompressionConfig,
    /// `Server-Timing` policy.
    pub server_timing: ServerTimingConfig,
    /// Security-header (helmet) policy.
    pub helmet: HelmetConfig,
    /// Cross-Site Request Forgery protection policy.
    pub csrf: CsrfConfig,
}

impl Default for HttpStackConfig {
    fn default() -> HttpStackConfig {
        HttpStackConfig {
            request_timeout: HumanDuration::from_secs(30),
            max_request_body: ByteSize::mib(64),
            forwarded_headers: ForwardedHeaderTrust::Untrusted,
            cors: CorsConfig::default(),
            compression: CompressionConfig::default(),
            decompression: DecompressionConfig::default(),
            server_timing: ServerTimingConfig::default(),
            helmet: HelmetConfig::default(),
            csrf: CsrfConfig::default(),
        }
    }
}

impl HttpStackConfig {
    /// Validate process-wide HTTP execution limits and CORS policy.
    ///
    /// # Errors
    ///
    /// Returns [`HttpStackConfigError::ZeroRequestTimeout`] when
    /// `request_timeout` is zero, [`HttpStackConfigError::ZeroRequestBodyLimit`]
    /// when `max_request_body` is zero, and
    /// [`HttpStackConfigError::CorsCredentialsWithWildcard`] when the CORS policy
    /// pairs credentials with an `any` wildcard.
    pub const fn validate(&self) -> Result<(), HttpStackConfigError> {
        if self.request_timeout.is_zero() {
            return Err(HttpStackConfigError::ZeroRequestTimeout);
        }
        if self.max_request_body.as_u64() == 0 {
            return Err(HttpStackConfigError::ZeroRequestBodyLimit);
        }
        if self.cors.has_credentialed_wildcard() {
            return Err(HttpStackConfigError::CorsCredentialsWithWildcard);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::config::{
        HttpStackConfig,
        HttpStackConfigError,
        cors::{CorsConfig, CorsMode},
        human_duration::HumanDuration,
    };
    use andromeda_http_headers::forwarded_header_trust::ForwardedHeaderTrust;
    use bytesize::ByteSize;
    use serde::Deserialize;
    use std::time::Duration;
    use toml_comment::TomlComment;

    #[derive(Deserialize)]
    struct BodyHolder {
        max_request_body: ByteSize,
    }

    #[derive(Deserialize)]
    struct TimeoutHolder {
        request_timeout: HumanDuration,
    }

    /// A broker that has not been told about a proxy must not believe one, so
    /// the declared default has to be the untrusted edge.
    #[test]
    fn forwarded_fields_are_untrusted_in_the_default_configuration() {
        pretty_assertions::assert_eq!(HttpStackConfig::default().forwarded_headers, ForwardedHeaderTrust::Untrusted);
    }

    #[test]
    fn zero_request_timeout_rejected() {
        let config = HttpStackConfig {
            request_timeout: HumanDuration::from_secs(0),
            ..HttpStackConfig::default()
        };
        pretty_assertions::assert_eq!(config.validate(), Err(HttpStackConfigError::ZeroRequestTimeout));
    }

    #[test]
    fn zero_body_limit_rejected() {
        let config = HttpStackConfig {
            max_request_body: ByteSize::b(0),
            ..HttpStackConfig::default()
        };
        pretty_assertions::assert_eq!(config.validate(), Err(HttpStackConfigError::ZeroRequestBodyLimit));
    }

    #[test]
    fn cors_credentials_with_wildcard_rejected() {
        let config = HttpStackConfig {
            cors: CorsConfig {
                mode: CorsMode::Enabled,
                allow_credentials: true,
                ..CorsConfig::default()
            },
            ..HttpStackConfig::default()
        };
        pretty_assertions::assert_eq!(config.validate(), Err(HttpStackConfigError::CorsCredentialsWithWildcard));
    }

    #[test]
    fn max_request_body_parses_human_readable() {
        let holder: BodyHolder = toml::from_str("max_request_body = \"64 MiB\"").expect("body size parses");
        pretty_assertions::assert_eq!(holder.max_request_body.as_u64(), 67_108_864);
    }

    #[test]
    fn request_timeout_parses_humantime() {
        let holder: TimeoutHolder = toml::from_str("request_timeout = \"30s\"").expect("timeout parses");
        pretty_assertions::assert_eq!(holder.request_timeout.duration(), Duration::from_secs(30));
    }

    #[test]
    fn default_toml_contains_nested_http_tables() {
        let rendered = <HttpStackConfig as TomlComment>::default_toml();
        for table in ["[cors]", "[compression]", "[decompression]", "[server_timing]", "[helmet]", "[csrf]"] {
            assert!(rendered.contains(table), "missing {table} in:\n{rendered}");
        }
        assert!(rendered.contains("request_timeout = \""), "{rendered}");
        assert!(rendered.contains("max_request_body = \""), "{rendered}");
    }

    #[test]
    fn valid_default_configuration_passes_validation() {
        assert!(HttpStackConfig::default().validate().is_ok());
    }
}
