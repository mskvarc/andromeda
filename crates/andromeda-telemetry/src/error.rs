//! Failures raised while installing the process recorder.

use metrics_exporter_prometheus::BuildError;
use thiserror::Error;

/// Why the metrics recorder could not be installed.
#[derive(Debug, Error)]
pub enum TelemetryError {
    /// The Prometheus recorder could not be built or registered as the global recorder.
    #[error("metrics recorder installation failed: {0}")]
    Recorder(#[from] BuildError),
}
