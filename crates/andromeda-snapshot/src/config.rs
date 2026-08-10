//! Snapshot operation configuration.

use confique::Config;
use serde::Serialize;
use thiserror::Error;

/// Invalid snapshot operation configuration.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum SnapshotConfigError {
    /// Snapshot preparation requires at least one execution slot.
    #[error("snapshot.preparation_concurrency must be at least 1")]
    ZeroPreparationConcurrency,
}

/// Snapshot preparation execution limits.
#[derive(Config, Debug, Clone, Serialize)]
pub struct SnapshotConfig {
    /// Maximum concurrent snapshot executions.
    #[config(env = "ANDROMEDA_SNAPSHOT_PREPARATION_CONCURRENCY", default = 4)]
    pub preparation_concurrency: usize,
}

impl SnapshotConfig {
    /// Validate snapshot preparation execution limits.
    pub const fn validate(&self) -> Result<(), SnapshotConfigError> {
        if self.preparation_concurrency == 0 {
            Err(SnapshotConfigError::ZeroPreparationConcurrency)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SnapshotConfig, SnapshotConfigError};

    #[test]
    fn zero_preparation_concurrency_is_rejected() {
        assert_eq!(
            SnapshotConfig { preparation_concurrency: 0 }.validate(),
            Err(SnapshotConfigError::ZeroPreparationConcurrency)
        );
    }
}
