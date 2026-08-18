//! Retention ages configured in whole seconds, as the durations the reaps subtract.

use crate::tenant_factory::error::TenantAssemblyError;

/// Turn a configured retention age in seconds into the signed duration a reap
/// subtracts from its cutoff instant.
///
/// `name` identifies the setting in the assembly failure a value beyond the
/// representable range produces; the configuration owner has already rejected
/// zero.
pub(super) fn retention_duration(seconds: u64, name: &'static str) -> Result<chrono::Duration, TenantAssemblyError> {
    let seconds = i64::try_from(seconds).map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    chrono::Duration::try_seconds(seconds).ok_or_else(|| TenantAssemblyError::invariant(format!("{name} is out of range")))
}
