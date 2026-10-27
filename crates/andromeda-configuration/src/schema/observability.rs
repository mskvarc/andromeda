//! The `[observability]` tables: logging and metrics.

use crate::schema::feature_mode::FeatureMode;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use strum::{Display, EnumString};
use toml_comment::TomlComment;

/// Broker log verbosity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, Display, EnumString)]
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

/// The on-disk shape of each structured log record.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, Display, EnumString)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum LogFileFormat {
    /// One JSON object per record, carrying the span context alongside the event.
    #[default]
    Json,
    /// One compact JSON object per line, the span context omitted (JSON Lines).
    Jsonl,
}

/// How often the file sink starts a new log file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, Display, EnumString)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum LogRotationInterval {
    /// Rotate once per calendar day.
    #[default]
    Daily,
    /// Rotate once per hour.
    Hourly,
}

/// Whether rotated log files are compressed, and with which codec.
///
/// The file sink is driven by `file-rotate`, which compresses only with gzip
/// (through `flate2`). A `Zstd` variant is intentionally absent: adding it would
/// require a different rotating writer rather than a flag, so the enum leaves
/// room for it without promising it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, Display, EnumString)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum LogCompression {
    /// Rotated files are kept uncompressed.
    None,
    /// Rotated files are gzip-compressed.
    #[default]
    Gzip,
}

/// Whether, and in which format, the broker exposes its metrics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, Display, EnumString)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum MetricsMode {
    /// No recorder is installed and no scrape endpoint is mounted.
    #[default]
    Disabled,
    /// A Prometheus recorder is installed and served at `GET /metrics` on the
    /// management plane.
    Prometheus,
}

/// The structured log file.
///
/// Off by default; on, a second tracing layer writes structured records to a
/// rotating, optionally compressed file, independent of standard output and
/// with its own verbosity.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LogFileConfig {
    /// Whether the structured file sink is installed.
    #[toml_comment(inline)]
    pub mode: FeatureMode,
    /// The directory the log files are written to. Left out, the platform
    /// state directory (falling back to the data directory) under an
    /// `andromeda` subdirectory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directory: Option<PathBuf>,
    /// The name of the active log file, before the rotation suffix.
    #[toml_comment(inline)]
    pub name: PathBuf,
    /// On-disk record format.
    #[toml_comment(inline)]
    pub format: LogFileFormat,
    /// How often a new log file is started.
    #[toml_comment(inline)]
    pub rotation: LogRotationInterval,
    /// Whether rotated files are compressed.
    #[toml_comment(inline)]
    pub compression: LogCompression,
    /// The most rotated files kept; the oldest is deleted past it.
    #[toml_comment(inline)]
    pub max_retained_files: usize,
    /// Maximum verbosity written to the file, independent of the stdout level.
    #[toml_comment(inline)]
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
#[derive(Deserialize, Serialize, Default, TomlComment, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LogConfig {
    /// The most verbose level written to standard output: `error`, `warn`,
    /// `info`, `debug` or `trace`.
    #[toml_comment(inline)]
    pub level: LogLevel,
    /// The structured log file.
    pub file: LogFileConfig,
}

/// Whether, and in which format, the broker exposes its metrics.
#[derive(Deserialize, Serialize, Default, TomlComment, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MetricsConfig {
    /// `disabled`, or `prometheus` to serve `GET /metrics` on the management
    /// plane.
    #[toml_comment(inline)]
    pub mode: MetricsMode,
}

/// Logging and metrics.
#[derive(Deserialize, Serialize, Default, TomlComment, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct ObservabilityConfig {
    /// What the broker logs, and where.
    pub log: LogConfig,
    /// Whether the broker exposes its metrics.
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
    use toml_comment::TomlComment;

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
        let rendered = ObservabilityConfig::default_toml();
        assert!(rendered.contains("[log.file]"), "{rendered}");
        assert!(rendered.contains("[metrics]"), "{rendered}");
        pretty_assertions::assert_eq!(ObservabilityConfig::default().metrics.mode, MetricsMode::Disabled);
    }
}
