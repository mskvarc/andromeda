//! Retention ages configured in whole seconds, as the durations the reaps subtract.

use crate::tenant_factory::assembly_invariant::{AssemblyInvariant, RetentionSetting};

/// Turn a configured retention age in seconds into the signed duration a reap
/// subtracts from its cutoff instant.
///
/// `setting` identifies the age in the assembly failure a value beyond the
/// representable range produces; the configuration owner has already rejected
/// zero.
pub(super) fn retention_duration(seconds: u64, setting: RetentionSetting) -> Result<chrono::Duration, AssemblyInvariant> {
    i64::try_from(seconds)
        .ok()
        .and_then(chrono::Duration::try_seconds)
        .ok_or(AssemblyInvariant::RetentionOutOfRange(setting))
}
