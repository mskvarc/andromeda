//! HTTP middleware configuration.

use confique::Config;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// Shared on/off switch for HTTP middleware features.
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

/// CORS mode for the broker's HTTP edge.
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

/// HTTP middleware configuration applied to all broker routes.
#[derive(Config, Debug, Clone, Serialize)]
pub struct HttpConfig {
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
}
