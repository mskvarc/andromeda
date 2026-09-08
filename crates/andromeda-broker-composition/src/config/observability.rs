//! Logging and metrics exposure configuration.

use confique::Config;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// Broker log verbosity.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, Display, EnumString)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum LogLevel {
    /// Error events only.
    Error,
    /// Warnings and errors.
    Warn,
    /// Operational information.
    #[default]
    Info,
    /// Debug diagnostics.
    Debug,
    /// Full trace diagnostics.
    Trace,
}

impl LogLevel {
    /// Convert into tracing's filter level.
    #[must_use]
    pub const fn as_tracing_level(self) -> tracing::Level {
        match self {
            LogLevel::Error => tracing::Level::ERROR,
            LogLevel::Warn => tracing::Level::WARN,
            LogLevel::Info => tracing::Level::INFO,
            LogLevel::Debug => tracing::Level::DEBUG,
            LogLevel::Trace => tracing::Level::TRACE,
        }
    }
}

/// Whether, and in which format, the broker exposes its metrics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, Display, EnumString)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum MetricsExposure {
    /// No recorder is installed and no scrape endpoint is mounted.
    #[default]
    Disabled,
    /// A Prometheus recorder is installed and served at `GET /metrics`.
    Prometheus,
}

/// Broker observability settings.
#[derive(Config, Debug, Clone, Serialize)]
pub struct ObservabilityConfig {
    /// Maximum emitted log verbosity.
    #[config(env = "ANDROMEDA_OBSERVABILITY_LOG_LEVEL", default = "info")]
    pub log_level: LogLevel,
    /// Metrics exposure mode.
    #[config(env = "ANDROMEDA_OBSERVABILITY_METRICS", default = "disabled")]
    pub metrics: MetricsExposure,
}
