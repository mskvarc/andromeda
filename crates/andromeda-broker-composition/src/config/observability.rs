//! Logging and metrics exposure configuration.

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

/// Whether the broker also writes a structured log to a file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, Display, EnumString)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum FileLoggingMode {
    /// No file sink is installed; only the stdout subscriber runs.
    #[default]
    Disabled,
    /// A structured file sink runs alongside the stdout subscriber.
    Enabled,
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

/// Structured file-logging configuration.
///
/// Disabled by default; when enabled it installs a second tracing layer writing
/// structured records to a rotating, optionally compressed file, independent of
/// the stdout subscriber and with its own verbosity.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone, PartialEq, Eq)]
pub struct FileLoggingConfig {
    /// Whether the structured file sink is installed.
    #[toml_comment(inline)]
    pub mode: FileLoggingMode,
    /// Directory the log files are written to.
    ///
    /// An empty value resolves to the platform state directory (falling back to
    /// the data directory), under an `andromeda` subdirectory.
    #[toml_comment(inline)]
    pub directory: PathBuf,
    /// Base file name of the active log file, before the rotation suffix.
    #[toml_comment(inline)]
    pub file_name: PathBuf,
    /// On-disk record format.
    #[toml_comment(inline)]
    pub format: LogFileFormat,
    /// How often a new log file is started.
    #[toml_comment(inline)]
    pub rotation: LogRotationInterval,
    /// Whether rotated files are compressed.
    #[toml_comment(inline)]
    pub compression: LogCompression,
    /// How many rotated files are retained before the oldest is deleted.
    #[toml_comment(inline)]
    pub retained_files: usize,
    /// Maximum verbosity written to the file, independent of the stdout level.
    #[toml_comment(inline)]
    pub level: LogLevel,
}

impl Default for FileLoggingConfig {
    // Manual rather than derived: the retained-file count, base file name and
    // default codec are non-trivial values a field-wise derive cannot express.
    fn default() -> FileLoggingConfig {
        FileLoggingConfig {
            mode: FileLoggingMode::Disabled,
            directory: PathBuf::new(),
            file_name: PathBuf::from("andromeda.log"),
            format: LogFileFormat::Json,
            rotation: LogRotationInterval::Daily,
            compression: LogCompression::Gzip,
            retained_files: 7,
            level: LogLevel::Info,
        }
    }
}

/// Broker observability settings.
#[derive(Deserialize, Serialize, Default, TomlComment, Debug, Clone)]
pub struct ObservabilityConfig {
    /// Maximum emitted log verbosity.
    #[toml_comment(inline)]
    pub log_level: LogLevel,
    /// Structured file-logging configuration.
    pub file_logging: FileLoggingConfig,
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::config::observability::{FileLoggingConfig, FileLoggingMode, LogCompression, LogFileFormat, LogRotationInterval, ObservabilityConfig};
    use std::str::FromStr;
    use toml_comment::TomlComment;

    #[test]
    fn file_logging_is_disabled_with_gzip_and_daily_retention_by_default() {
        let config = FileLoggingConfig::default();
        pretty_assertions::assert_eq!(config.mode, FileLoggingMode::Disabled);
        pretty_assertions::assert_eq!(config.compression, LogCompression::Gzip);
        pretty_assertions::assert_eq!(config.format, LogFileFormat::Json);
        pretty_assertions::assert_eq!(config.rotation, LogRotationInterval::Daily);
        pretty_assertions::assert_eq!(config.retained_files, 7);
    }

    #[test]
    fn the_new_enums_round_trip_kebab_case_through_serde_and_from_str() {
        assert_eq!(serde_json::to_string(&FileLoggingMode::Enabled).expect("serialises"), "\"enabled\"");
        assert_eq!(serde_json::to_string(&LogFileFormat::Jsonl).expect("serialises"), "\"jsonl\"");
        assert_eq!(serde_json::to_string(&LogRotationInterval::Hourly).expect("serialises"), "\"hourly\"");
        assert_eq!(serde_json::to_string(&LogCompression::None).expect("serialises"), "\"none\"");
        assert_eq!(FileLoggingMode::from_str("enabled").expect("parses"), FileLoggingMode::Enabled);
        assert_eq!(LogFileFormat::from_str("jsonl").expect("parses"), LogFileFormat::Jsonl);
        assert_eq!(LogRotationInterval::from_str("hourly").expect("parses"), LogRotationInterval::Hourly);
        assert_eq!(LogCompression::from_str("gzip").expect("parses"), LogCompression::Gzip);
    }

    #[test]
    fn the_default_toml_carries_the_file_logging_section() {
        let rendered = ObservabilityConfig::default_toml();
        assert!(rendered.contains("file_logging"), "{rendered}");
    }
}
