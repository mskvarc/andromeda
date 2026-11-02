//! Failures raised while installing the process recorder.

use metrics_exporter_prometheus::BuildError;
use thiserror::Error;

/// Why the metrics recorder could not be installed.
#[derive(Debug, Error)]
pub enum TelemetryError {
    /// The Prometheus recorder could not be built.
    #[error("metrics recorder installation failed: {0}")]
    Recorder(#[from] BuildError),
    /// Another recorder already holds the process-wide sink.
    #[error("another metrics recorder is already installed")]
    AlreadyInstalled,
}
