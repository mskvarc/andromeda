//! The `[observability]` tables: logging and metrics.

use crate::schema::feature_mode::FeatureMode;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use strum::{Display, EnumString};

/// Broker log verbosity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, Display, EnumString, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
#[schemars(description = "The most verbose log level written.")]
pub enum LogLevel {
    /// Error events only.
    #[schemars(description = "Writes errors only.")]
    Error,
    /// Warnings and errors.
    #[schemars(description = "Writes warnings and errors.")]
    Warn,
    /// Operational information.
    #[default]
    #[schemars(description = "Writes operational information, warnings and errors.")]
    Info,
    /// Debug diagnostics.
    #[schemars(description = "Adds debug diagnostics.")]
    Debug,
    /// Full trace diagnostics.
    #[schemars(description = "Adds full trace diagnostics.")]
    Trace,
}

/// The on-disk shape of each structured log record.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, Display, EnumString, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
#[schemars(description = "The shape of each record in the log file.")]
pub enum LogFileFormat {
    /// One JSON object per record, carrying the span context alongside the event.
    #[default]
    #[schemars(description = "One JSON record per line, with the current span and the span list.")]
    Json,
    /// One compact JSON object per line, the span context omitted (JSON Lines).
    #[schemars(description = "One compact JSON record per line, without the span fields (JSON Lines).")]
    Jsonl,
}

/// How often the file sink starts a new log file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, Display, EnumString, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
#[schemars(description = "How often the log file is rotated.")]
pub enum LogRotationInterval {
    /// Rotate once per calendar day.
    #[default]
    #[schemars(description = "Starts a new file once per calendar day.")]
    Daily,
    /// Rotate once per hour.
    #[schemars(description = "Starts a new file once per hour.")]
    Hourly,
}

/// Whether rotated log files are compressed, and with which codec.
///
/// The file sink is driven by `file-rotate`, which compresses only with gzip
/// (through `flate2`). A `Zstd` variant is intentionally absent: adding it would
/// require a different rotating writer rather than a flag, so the enum leaves
/// room for it without promising it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, Display, EnumString, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
#[schemars(description = "Whether rotated log files are compressed.")]
pub enum LogCompression {
    /// Rotated files are kept uncompressed.
    #[schemars(description = "Keeps rotated files uncompressed.")]
    None,
    /// Rotated files are gzip-compressed.
    #[default]
    #[schemars(description = "Compresses rotated files with gzip.")]
    Gzip,
}

/// Whether, and in which format, the broker exposes its metrics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, Display, EnumString, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
#[schemars(description = "Whether, and in which format, the broker exposes metrics.")]
pub enum MetricsMode {
    /// No recorder is installed and no scrape endpoint is mounted.
    #[default]
    #[schemars(description = "Records no metrics and serves no `/metrics` endpoint.")]
    Disabled,
    /// A Prometheus recorder is installed and served at `GET /metrics` on the
    /// management plane.
    #[schemars(description = "Serves Prometheus metrics at `GET /metrics` on the management endpoints.")]
    Prometheus,
}

/// The structured log file.
///
/// Off by default; on, a second tracing layer writes structured records to a
/// rotating, optionally compressed file, independent of standard output and
/// with its own verbosity.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[schemars(description = "The structured log file, written in addition to standard output with a level of its own. `RUST_LOG` does not affect it.")]
pub struct LogFileConfig {
    /// Whether the structured file sink is installed.
    #[schemars(description = "Whether structured records are written to a rotating file.")]
    pub mode: FeatureMode,
    /// The directory the log files are written to. Left out, the platform
    /// state directory (falling back to the data directory) under an
    /// `andromeda` subdirectory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        title = "path",
        description = "The directory the log files go in. Left out, an `andromeda` directory in the platform state directory (`$XDG_STATE_HOME`, or `~/.local/state`), or in the data directory on a platform without one."
    )]
    pub directory: Option<PathBuf>,
    /// The name of the active log file, before the rotation suffix.
    #[schemars(title = "path", description = "The name of the current log file, before the rotation suffix.")]
    pub name: PathBuf,
    /// On-disk record format.
    #[schemars(description = "The shape of each record in the file.")]
    pub format: LogFileFormat,
    /// How often a new log file is started.
    #[schemars(description = "How often a new file is started.")]
    pub rotation: LogRotationInterval,
    /// Whether rotated files are compressed.
    #[schemars(description = "Whether rotated files are compressed.")]
    pub compression: LogCompression,
    /// The most rotated files kept; the oldest is deleted past it.
    #[schemars(description = "The most rotated files kept. Past it, the oldest is deleted.")]
    pub max_retained_files: usize,
    /// Maximum verbosity written to the file, independent of the stdout level.
    #[schemars(description = "The most verbose level written to the file, independent of `observability.log.level`.")]
    pub level: LogLevel,
}

impl Default for LogFileConfig {
    // Manual rather than derived: the retained-file count, file name and
    // default codec are non-trivial values a field-wise derive cannot express.
    fn default() -> LogFileConfig {
        LogFileConfig {
            mode: FeatureMode::Disabled,
            directory: None,
            name: PathBuf::from("andromeda.log"),
            format: LogFileFormat::Json,
            rotation: LogRotationInterval::Daily,
            compression: LogCompression::Gzip,
            max_retained_files: 7,
            level: LogLevel::Info,
        }
    }
}

/// What the broker logs, and where.
#[derive(Deserialize, Serialize, Default, JsonSchema, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[schemars(description = "What the broker logs, and where.")]
pub struct LogConfig {
    /// The most verbose level written to standard output: `error`, `warn`,
    /// `info`, `debug` or `trace`.
    #[schemars(description = "The most verbose level written to standard output. A `RUST_LOG` environment variable takes precedence when it is set.")]
    pub level: LogLevel,
    /// The structured log file.
    #[schemars(description = "The optional structured log file.")]
    pub file: LogFileConfig,
}

/// Whether, and in which format, the broker exposes its metrics.
#[derive(Deserialize, Serialize, Default, JsonSchema, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[schemars(description = "Whether, and in which format, the broker exposes metrics.")]
pub struct MetricsConfig {
    /// `disabled`, or `prometheus` to serve `GET /metrics` on the management
    /// plane.
    #[schemars(description = "Whether metrics are recorded and served on the management endpoints.")]
    pub mode: MetricsMode,
}

/// Logging and metrics.
#[derive(Deserialize, Serialize, Default, JsonSchema, Debug, Clone)]
#[serde(deny_unknown_fields)]
#[schemars(description = "Logging and metrics.")]
pub struct ObservabilityConfig {
    /// What the broker logs, and where.
    #[schemars(description = "What the broker logs, and where.")]
    pub log: LogConfig,
    /// Whether the broker exposes its metrics.
    #[schemars(description = "Whether the broker exposes metrics.")]
    pub metrics: MetricsConfig,
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::schema::{
        feature_mode::FeatureMode,
        observability::{LogCompression, LogFileConfig, LogFileFormat, LogRotationInterval, MetricsMode, ObservabilityConfig},
    };
    use std::str::FromStr;

    #[test]
    fn file_logging_is_disabled_with_gzip_and_daily_retention_by_default() {
        let config = LogFileConfig::default();
        pretty_assertions::assert_eq!(config.mode, FeatureMode::Disabled);
        pretty_assertions::assert_eq!(config.compression, LogCompression::Gzip);
        pretty_assertions::assert_eq!(config.format, LogFileFormat::Json);
        pretty_assertions::assert_eq!(config.rotation, LogRotationInterval::Daily);
        pretty_assertions::assert_eq!(config.max_retained_files, 7);
        pretty_assertions::assert_eq!(config.directory, None);
    }

    #[test]
    fn the_new_enums_round_trip_kebab_case_through_serde_and_from_str() {
        assert_eq!(serde_json::to_string(&FeatureMode::Enabled).expect("serialises"), "\"enabled\"");
        assert_eq!(serde_json::to_string(&LogFileFormat::Jsonl).expect("serialises"), "\"jsonl\"");
        assert_eq!(serde_json::to_string(&LogRotationInterval::Hourly).expect("serialises"), "\"hourly\"");
        assert_eq!(serde_json::to_string(&LogCompression::None).expect("serialises"), "\"none\"");
        assert_eq!(FeatureMode::from_str("enabled").expect("parses"), FeatureMode::Enabled);
        assert_eq!(LogFileFormat::from_str("jsonl").expect("parses"), LogFileFormat::Jsonl);
        assert_eq!(LogRotationInterval::from_str("hourly").expect("parses"), LogRotationInterval::Hourly);
        assert_eq!(LogCompression::from_str("gzip").expect("parses"), LogCompression::Gzip);
    }

    #[test]
    fn the_default_toml_carries_the_file_logging_section() {
        let rendered = toml::to_string(&ObservabilityConfig::default()).expect("the defaults render");
        assert!(rendered.contains("[log.file]"), "{rendered}");
        assert!(rendered.contains("[metrics]"), "{rendered}");
        pretty_assertions::assert_eq!(ObservabilityConfig::default().metrics.mode, MetricsMode::Disabled);
    }
}
