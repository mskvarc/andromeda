//! Configuration for process-wide inbound HTTP mechanics.

use andromeda_http_headers::forwarded_header_trust::ForwardedHeaderTrust;
use confique::Config;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};
use thiserror::Error;

/// Invalid process-wide HTTP stack configuration.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum HttpStackConfigError {
    /// A request requires a positive timeout.
    #[error("http.request_timeout_secs must be at least 1")]
    ZeroRequestTimeout,
    /// Request bodies require a positive size ceiling.
    #[error("http.max_request_body_bytes must be at least 1")]
    ZeroRequestBodyLimit,
}

/// Shared on/off switch for an HTTP stack feature.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, Display, EnumString, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum HttpFeatureMode {
    /// Disable the feature.
    Disabled,
    /// Enable the feature.
    #[default]
    Enabled,
}

/// Browser origin policy at the broker edge.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, Display, EnumString, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum CorsMode {
    /// Do not install CORS middleware.
    Disabled,
    /// Accept browser requests from every origin.
    #[default]
    Permissive,
}

/// Process-wide inbound HTTP stack configuration.
#[derive(Config, Debug, Clone, Serialize)]
pub struct HttpStackConfig {
    /// Maximum wall-clock time for one HTTP request.
    #[config(env = "ANDROMEDA_HTTP_REQUEST_TIMEOUT_SECS", default = 30)]
    pub request_timeout_secs: u64,
    /// Whether eligible HTTP responses are compressed.
    #[config(env = "ANDROMEDA_HTTP_RESPONSE_COMPRESSION", default = "enabled")]
    pub response_compression: HttpFeatureMode,
    /// Whether compressed HTTP request bodies are accepted.
    #[config(env = "ANDROMEDA_HTTP_REQUEST_DECOMPRESSION", default = "enabled")]
    pub request_decompression: HttpFeatureMode,
    /// CORS policy for browser-facing clients.
    #[config(env = "ANDROMEDA_HTTP_CORS", default = "permissive")]
    pub cors: CorsMode,
    /// Maximum accepted HTTP request body size.
    #[config(env = "ANDROMEDA_HTTP_MAX_REQUEST_BODY_BYTES", default = 67_108_864)]
    pub max_request_body_bytes: usize,
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
    #[config(env = "ANDROMEDA_HTTP_FORWARDED_HEADERS", default = "untrusted")]
    pub forwarded_headers: ForwardedHeaderTrust,
}

impl HttpStackConfig {
    /// Validate process-wide HTTP execution limits.
    ///
    /// # Errors
    ///
    /// Returns [`HttpStackConfigError::ZeroRequestTimeout`] when
    /// `request_timeout_secs` is zero, and
    /// [`HttpStackConfigError::ZeroRequestBodyLimit`] when
    /// `max_request_body_bytes` is zero.
    pub const fn validate(&self) -> Result<(), HttpStackConfigError> {
        if self.request_timeout_secs == 0 {
            return Err(HttpStackConfigError::ZeroRequestTimeout);
        }
        if self.max_request_body_bytes == 0 {
            return Err(HttpStackConfigError::ZeroRequestBodyLimit);
        }
        Ok(())
    }
}

#[cfg(test)]
mod config_tests {
    use super::{CorsMode, HttpFeatureMode, HttpStackConfig, HttpStackConfigError};
    use andromeda_http_headers::forwarded_header_trust::ForwardedHeaderTrust;
    use confique::Config as _;

    fn valid_config() -> HttpStackConfig {
        HttpStackConfig {
            request_timeout_secs: 30,
            response_compression: HttpFeatureMode::Enabled,
            request_decompression: HttpFeatureMode::Enabled,
            cors: CorsMode::Permissive,
            max_request_body_bytes: 1024,
            forwarded_headers: ForwardedHeaderTrust::Untrusted,
        }
    }

    /// A broker that has not been told about a proxy must not believe one, so
    /// the declared default has to be the untrusted edge.
    #[test]
    fn forwarded_fields_are_untrusted_in_the_default_configuration() -> Result<(), confique::Error> {
        let config = HttpStackConfig::builder().load()?;
        pretty_assertions::assert_eq!(config.forwarded_headers, ForwardedHeaderTrust::Untrusted);
        Ok(())
    }

    #[test]
    fn zero_request_body_limit_is_rejected() {
        let mut config = valid_config();
        config.max_request_body_bytes = 0;
        assert_eq!(config.validate(), Err(HttpStackConfigError::ZeroRequestBodyLimit));
    }
}
