//! Configuration for process-wide inbound HTTP mechanics.

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
}

impl HttpStackConfig {
    /// Validate process-wide HTTP execution limits.
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

    fn valid_config() -> HttpStackConfig {
        HttpStackConfig {
            request_timeout_secs: 30,
            response_compression: HttpFeatureMode::Enabled,
            request_decompression: HttpFeatureMode::Enabled,
            cors: CorsMode::Permissive,
            max_request_body_bytes: 1024,
        }
    }

    #[test]
    fn zero_request_body_limit_is_rejected() {
        let mut config = valid_config();
        config.max_request_body_bytes = 0;
        assert_eq!(config.validate(), Err(HttpStackConfigError::ZeroRequestBodyLimit));
    }
}
