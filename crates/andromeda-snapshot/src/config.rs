//! Snapshot operation configuration.

use andromeda_configuration_scalars::{human_byte_size::HumanByteSize, human_duration::HumanDuration};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use toml_comment::TomlComment;

/// Invalid snapshot operation configuration.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum SnapshotConfigError {
    /// The configured default lifetime must be positive.
    #[error("snapshot.default_lifetime must be greater than zero")]
    ZeroDefaultLifetime,
    /// The configured maximum lifetime must be positive.
    #[error("snapshot.maximum_lifetime must be greater than zero")]
    ZeroMaximumLifetime,
    /// The default lifetime cannot exceed the configured maximum.
    #[error("snapshot.default_lifetime must not exceed snapshot.maximum_lifetime")]
    DefaultLifetimeExceedsMaximum,
    /// Capture paging requires at least one result per page.
    #[error("snapshot.capture_page_size must be at least 1")]
    ZeroCapturePageSize,
    /// Snapshot preparation requires at least one execution slot.
    #[error("snapshot.preparation_concurrency must be at least 1")]
    ZeroPreparationConcurrency,
    /// Scoped operations require a positive bounded wait.
    #[error("snapshot.scoped_operation_wait must be greater than zero")]
    ZeroScopedOperationWait,
    /// Preparation claims require a positive lease.
    #[error("snapshot.preparation_lease must be greater than zero")]
    ZeroPreparationLease,
    /// The preparation recovery cadence must be positive.
    #[error("snapshot.preparation_recovery must be greater than zero")]
    ZeroPreparationRecovery,
    /// An enabled count quota must allow at least one Snapshot.
    #[error("snapshot.tenant_count_quota must be at least 1 when configured")]
    ZeroTenantCountQuota,
    /// An enabled storage quota must allow at least one byte.
    #[error("snapshot.tenant_storage_quota must be at least 1 byte when configured")]
    ZeroTenantStorageQuota,
}

/// Snapshot preparation execution limits.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone)]
pub struct SnapshotConfig {
    /// Default lifetime used when `snapshotLifetime` is omitted, in
    /// human-readable form (`"1h"`).
    #[toml_comment(inline)]
    pub default_lifetime: HumanDuration,
    /// Maximum accepted Snapshot lifetime, in human-readable form (`"30days"`).
    #[toml_comment(inline)]
    pub maximum_lifetime: HumanDuration,
    /// Number of results fetched in each capture page.
    pub capture_page_size: usize,
    /// Maximum concurrent snapshot executions.
    pub preparation_concurrency: usize,
    /// Maximum time a scoped operation waits for capture or clone promotion, in
    /// human-readable form (`"30s"`).
    #[toml_comment(inline)]
    pub scoped_operation_wait: HumanDuration,
    /// Lease duration for a claimed preparation job, in human-readable form
    /// (`"30s"`).
    #[toml_comment(inline)]
    pub preparation_lease: HumanDuration,
    /// Cadence of the recovery sweep that prepares claimed-but-unfinished
    /// Snapshots, in human-readable form (`"30s"`).
    ///
    /// Creation prepares its Snapshot immediately, so this sweep only recovers a
    /// preparation a crash or missed run left behind; it need not be frequent.
    #[toml_comment(inline)]
    pub preparation_recovery: HumanDuration,
    /// Optional maximum number of Snapshots retained for a tenant.
    pub tenant_count_quota: Option<usize>,
    /// Optional maximum Snapshot storage retained for a tenant, in
    /// human-readable form (`"1 GiB"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant_storage_quota: Option<HumanByteSize>,
}

impl Default for SnapshotConfig {
    fn default() -> SnapshotConfig {
        SnapshotConfig {
            default_lifetime: HumanDuration::from_secs(3_600),
            maximum_lifetime: HumanDuration::from_secs(2_592_000),
            capture_page_size: 250,
            preparation_concurrency: 4,
            scoped_operation_wait: HumanDuration::from_secs(30),
            preparation_lease: HumanDuration::from_secs(30),
            preparation_recovery: HumanDuration::from_secs(30),
            tenant_count_quota: None,
            tenant_storage_quota: None,
        }
    }
}

impl SnapshotConfig {
    /// Validate snapshot preparation execution limits.
    ///
    /// # Errors
    ///
    /// Returns the matching [`SnapshotConfigError`] when a lifetime, capture
    /// page size, preparation concurrency, scoped-operation wait or preparation
    /// lease is zero, when the default lifetime exceeds the maximum lifetime, or
    /// when a configured tenant count or byte quota is zero.
    pub fn validate(&self) -> Result<(), SnapshotConfigError> {
        if self.default_lifetime.is_zero() {
            Err(SnapshotConfigError::ZeroDefaultLifetime)
        } else if self.maximum_lifetime.is_zero() {
            Err(SnapshotConfigError::ZeroMaximumLifetime)
        } else if self.default_lifetime.duration().as_secs() > self.maximum_lifetime.duration().as_secs() {
            Err(SnapshotConfigError::DefaultLifetimeExceedsMaximum)
        } else if self.capture_page_size == 0 {
            Err(SnapshotConfigError::ZeroCapturePageSize)
        } else if self.preparation_concurrency == 0 {
            Err(SnapshotConfigError::ZeroPreparationConcurrency)
        } else if self.scoped_operation_wait.is_zero() {
            Err(SnapshotConfigError::ZeroScopedOperationWait)
        } else if self.preparation_lease.is_zero() {
            Err(SnapshotConfigError::ZeroPreparationLease)
        } else if self.preparation_recovery.is_zero() {
            Err(SnapshotConfigError::ZeroPreparationRecovery)
        } else if self.tenant_count_quota == Some(0) {
            Err(SnapshotConfigError::ZeroTenantCountQuota)
        } else if self.tenant_storage_quota == Some(HumanByteSize::b(0)) {
            Err(SnapshotConfigError::ZeroTenantStorageQuota)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SnapshotConfig, SnapshotConfigError};
    use andromeda_configuration_scalars::human_duration::HumanDuration;
    use pretty_assertions::assert_eq;

    #[test]
    fn zero_preparation_concurrency_is_rejected() {
        assert_eq!(
            SnapshotConfig {
                default_lifetime: HumanDuration::from_secs(3_600),
                maximum_lifetime: HumanDuration::from_secs(2_592_000),
                capture_page_size: 250,
                preparation_concurrency: 0,
                scoped_operation_wait: HumanDuration::from_secs(30),
                preparation_lease: HumanDuration::from_secs(30),
                preparation_recovery: HumanDuration::from_secs(30),
                tenant_count_quota: None,
                tenant_storage_quota: None,
            }
            .validate(),
            Err(SnapshotConfigError::ZeroPreparationConcurrency)
        );
    }

    #[test]
    fn zero_scoped_operation_wait_is_rejected() {
        assert_eq!(
            SnapshotConfig {
                default_lifetime: HumanDuration::from_secs(3_600),
                maximum_lifetime: HumanDuration::from_secs(2_592_000),
                capture_page_size: 250,
                preparation_concurrency: 4,
                scoped_operation_wait: HumanDuration::from_secs(0),
                preparation_lease: HumanDuration::from_secs(30),
                preparation_recovery: HumanDuration::from_secs(30),
                tenant_count_quota: None,
                tenant_storage_quota: None,
            }
            .validate(),
            Err(SnapshotConfigError::ZeroScopedOperationWait)
        );
    }
}
