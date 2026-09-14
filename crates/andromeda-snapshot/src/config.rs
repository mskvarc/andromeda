//! Snapshot operation configuration.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use toml_comment::TomlComment;

/// Invalid snapshot operation configuration.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum SnapshotConfigError {
    /// The configured default lifetime must be positive.
    #[error("snapshot.default_lifetime_secs must be at least 1")]
    ZeroDefaultLifetime,
    /// The configured maximum lifetime must be positive.
    #[error("snapshot.maximum_lifetime_secs must be at least 1")]
    ZeroMaximumLifetime,
    /// The default lifetime cannot exceed the configured maximum.
    #[error("snapshot.default_lifetime_secs must not exceed snapshot.maximum_lifetime_secs")]
    DefaultLifetimeExceedsMaximum,
    /// Capture paging requires at least one result per page.
    #[error("snapshot.capture_page_size must be at least 1")]
    ZeroCapturePageSize,
    /// Snapshot preparation requires at least one execution slot.
    #[error("snapshot.preparation_concurrency must be at least 1")]
    ZeroPreparationConcurrency,
    /// Scoped operations require a positive bounded wait.
    #[error("snapshot.scoped_operation_wait_secs must be at least 1")]
    ZeroScopedOperationWait,
    /// Preparation claims require a positive lease.
    #[error("snapshot.preparation_lease_secs must be at least 1")]
    ZeroPreparationLease,
    /// An enabled count quota must allow at least one Snapshot.
    #[error("snapshot.tenant_count_quota must be at least 1 when configured")]
    ZeroTenantCountQuota,
    /// An enabled byte quota must allow at least one byte.
    #[error("snapshot.tenant_byte_quota must be at least 1 when configured")]
    ZeroTenantByteQuota,
}

/// Snapshot preparation execution limits.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone)]
pub struct SnapshotConfig {
    /// Default lifetime used when `snapshotLifetime` is omitted.
    pub default_lifetime_secs: u64,
    /// Maximum accepted Snapshot lifetime.
    pub maximum_lifetime_secs: u64,
    /// Number of results fetched in each capture page.
    pub capture_page_size: usize,
    /// Maximum concurrent snapshot executions.
    pub preparation_concurrency: usize,
    /// Maximum time a scoped operation waits for capture or clone promotion.
    pub scoped_operation_wait_secs: u64,
    /// Lease duration for a claimed preparation job.
    pub preparation_lease_secs: u64,
    /// Optional maximum number of Snapshots retained for a tenant.
    pub tenant_count_quota: Option<usize>,
    /// Optional maximum number of Snapshot bytes retained for a tenant.
    pub tenant_byte_quota: Option<u64>,
}

impl Default for SnapshotConfig {
    fn default() -> SnapshotConfig {
        SnapshotConfig {
            default_lifetime_secs: 3_600,
            maximum_lifetime_secs: 2_592_000,
            capture_page_size: 250,
            preparation_concurrency: 4,
            scoped_operation_wait_secs: 30,
            preparation_lease_secs: 30,
            tenant_count_quota: None,
            tenant_byte_quota: None,
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
        if self.default_lifetime_secs == 0 {
            Err(SnapshotConfigError::ZeroDefaultLifetime)
        } else if self.maximum_lifetime_secs == 0 {
            Err(SnapshotConfigError::ZeroMaximumLifetime)
        } else if self.default_lifetime_secs > self.maximum_lifetime_secs {
            Err(SnapshotConfigError::DefaultLifetimeExceedsMaximum)
        } else if self.capture_page_size == 0 {
            Err(SnapshotConfigError::ZeroCapturePageSize)
        } else if self.preparation_concurrency == 0 {
            Err(SnapshotConfigError::ZeroPreparationConcurrency)
        } else if self.scoped_operation_wait_secs == 0 {
            Err(SnapshotConfigError::ZeroScopedOperationWait)
        } else if self.preparation_lease_secs == 0 {
            Err(SnapshotConfigError::ZeroPreparationLease)
        } else if self.tenant_count_quota == Some(0) {
            Err(SnapshotConfigError::ZeroTenantCountQuota)
        } else if self.tenant_byte_quota == Some(0) {
            Err(SnapshotConfigError::ZeroTenantByteQuota)
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
            SnapshotConfig {
                default_lifetime_secs: 3_600,
                maximum_lifetime_secs: 2_592_000,
                capture_page_size: 250,
                preparation_concurrency: 0,
                scoped_operation_wait_secs: 30,
                preparation_lease_secs: 30,
                tenant_count_quota: None,
                tenant_byte_quota: None,
            }
            .validate(),
            Err(SnapshotConfigError::ZeroPreparationConcurrency)
        );
    }

    #[test]
    fn zero_scoped_operation_wait_is_rejected() {
        assert_eq!(
            SnapshotConfig {
                default_lifetime_secs: 3_600,
                maximum_lifetime_secs: 2_592_000,
                capture_page_size: 250,
                preparation_concurrency: 4,
                scoped_operation_wait_secs: 0,
                preparation_lease_secs: 30,
                tenant_count_quota: None,
                tenant_byte_quota: None,
            }
            .validate(),
            Err(SnapshotConfigError::ZeroScopedOperationWait)
        );
    }
}
