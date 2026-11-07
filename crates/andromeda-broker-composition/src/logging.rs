//! Composition-owned tracing subscriber installation.
//!
//! The broker always logs a compact stream to stdout and, when configured, a
//! second structured stream to a rotating file. Both are assembled and installed
//! here, ahead of any runtime resource being opened.

use andromeda_configuration::{
    broker_config::BrokerConfig,
    schema::{
        feature_mode::FeatureMode,
        observability::{LogCompression, LogFileConfig, LogFileFormat, LogLevel, LogRotationInterval},
    },
};
use etcetera::{BaseStrategy, choose_base_strategy};
use file_rotate::{
    ContentLimit,
    FileRotate,
    TimeFrequency,
    compression::Compression,
    suffix::{AppendTimestamp, FileLimit},
};
use std::{
    fs,
    io::Error as IoError,
    path::{Path, PathBuf},
};
use thiserror::Error;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{
    EnvFilter,
    Layer,
    Registry,
    fmt,
    layer::SubscriberExt,
    util::{SubscriberInitExt, TryInitError},
};

/// Failure while installing composition-owned tracing.
#[derive(Debug, Error)]
pub(crate) enum TracingInitError {
    /// The configured log directory could not be created.
    #[error("the log directory {} could not be created: {source}", .directory.display())]
    LogDirectory {
        /// The directory whose creation failed.
        directory: PathBuf,
        /// The underlying filesystem error.
        source: IoError,
    },
    /// A tracing subscriber was already installed for this process.
    #[error(transparent)]
    Install(#[from] TryInitError),
}

/// Install composition-owned tracing before any runtime resource is opened.
///
/// A compact stdout layer is always installed, honouring `RUST_LOG` and falling
/// back to the configured level. When file logging is enabled a second,
/// independently filtered structured layer writes to a rotating file; its
/// [`WorkerGuard`] is returned so the caller can keep the non-blocking appender
/// alive for the process lifetime.
///
/// # Errors
///
/// Returns [`TracingInitError::LogDirectory`] when the file sink's directory
/// cannot be created, and [`TracingInitError::Install`] when a subscriber is
/// already installed for the process.
pub(crate) fn initialize(config: &BrokerConfig) -> Result<Option<WorkerGuard>, TracingInitError> {
    let log = &config.observability.log;
    let stdout_layer = fmt::layer().compact().with_filter(stdout_env_filter(log.level));

    let (file_layer, guard) = match log.file.mode {
        FeatureMode::Disabled => (None, None),
        FeatureMode::Enabled => {
            let (layer, guard) = build_file_layer(&log.file)?;
            (Some(layer), Some(guard))
        }
    };

    Registry::default().with(file_layer).with(stdout_layer).try_init()?;
    Ok(guard)
}

/// The stdout filter: `RUST_LOG` when set, otherwise the configured level.
fn stdout_env_filter(level: LogLevel) -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level.to_string()))
}

/// Build the structured file-logging layer and its flush guard.
///
/// The layer emits JSON records to a [`FileRotate`] writer wrapped in a
/// non-blocking appender, filtered at the file sink's own configured level
/// independently of `RUST_LOG`.
fn build_file_layer(config: &LogFileConfig) -> Result<(Box<dyn Layer<Registry> + Send + Sync>, WorkerGuard), TracingInitError> {
    let directory = resolve_log_directory(config.directory.as_deref());
    // Created ahead of `FileRotate::new`, which would otherwise panic internally
    // if the directory could not be made; here the failure is a typed error.
    fs::create_dir_all(&directory).map_err(|source| TracingInitError::LogDirectory {
        directory: directory.clone(), // clone: the error keeps the path the writer also needs
        source,
    })?;
    let frequency = match config.rotation {
        LogRotationInterval::Daily => TimeFrequency::Daily,
        LogRotationInterval::Hourly => TimeFrequency::Hourly,
    };
    // `OnRotate(0)` compresses every rotated file, keeping only the live one raw.
    let compression = match config.compression {
        LogCompression::None => Compression::None,
        LogCompression::Gzip => Compression::OnRotate(0),
    };
    let writer = FileRotate::new(
        directory.join(&config.name),
        AppendTimestamp::default(FileLimit::MaxFiles(config.max_retained_files.get())),
        ContentLimit::Time(frequency),
        compression,
        None,
    );
    let (writer, guard) = tracing_appender::non_blocking(writer);
    let filter = EnvFilter::new(config.level.to_string());
    let layer = fmt::layer().json().with_ansi(false).with_writer(writer);
    let layer = match config.format {
        LogFileFormat::Json => layer.with_span_list(true).with_current_span(true).with_filter(filter).boxed(),
        LogFileFormat::Jsonl => layer.with_span_list(false).with_current_span(false).with_filter(filter).boxed(),
    };
    Ok((layer, guard))
}

/// Resolve the directory the file sink writes to.
///
/// A directory left out falls back to the platform state directory, or the
/// data directory where no state directory exists, under `andromeda`.
fn resolve_log_directory(configured: Option<&Path>) -> PathBuf {
    if let Some(configured) = configured {
        return configured.to_path_buf();
    }
    choose_base_strategy()
        .ok()
        .map(|strategy| strategy.state_dir().unwrap_or_else(|| strategy.data_dir()))
        .map_or_else(|| PathBuf::from("andromeda-logs"), |base| base.join("andromeda"))
}

#[cfg(test)]
mod tests {
    use crate::logging::resolve_log_directory;
    use andromeda_configuration::schema::observability::LogFileConfig;
    use std::path::{Path, PathBuf};

    #[test]
    fn an_explicit_directory_is_used_verbatim() {
        let directory = resolve_log_directory(Some(Path::new("/var/log/andromeda")));
        pretty_assertions::assert_eq!(directory, PathBuf::from("/var/log/andromeda"));
    }

    #[test]
    fn a_directory_left_out_resolves_to_a_platform_default_under_andromeda() {
        let directory = resolve_log_directory(LogFileConfig::default().directory.as_deref());
        assert!(
            directory.ends_with("andromeda") || directory == Path::new("andromeda-logs"),
            "unexpected default log directory: {directory:?}"
        );
    }
}
