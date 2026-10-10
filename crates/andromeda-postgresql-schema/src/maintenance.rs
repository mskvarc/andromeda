//! Platform-database relations that retain PostgreSQL maintenance state.

use sea_query::Iden;

/// Per-tenant inspection ordering and failure state.
#[derive(Clone, Copy, Iden)]
pub enum PostgresqlMaintenanceTenants {
    /// Table name: `postgresql_maintenance_tenants`.
    Table,
    /// Foreign key to the platform tenant registry.
    TenantKey,
    /// Most recent bounded inspection pass.
    LastInspectedAt,
    /// Consecutive database-reachability failures.
    ConsecutiveFailures,
    /// Last-pass outcome (`MaintenanceInspectionResultCode`).
    LastResult,
}

/// Per-target degradation evidence, pending work and action results.
#[derive(Clone, Copy, Iden)]
pub enum PostgresqlMaintenanceTargets {
    /// Table name: `postgresql_maintenance_targets`.
    Table,
    /// Foreign key to the platform tenant registry.
    TenantKey,
    /// Closed-set relation (`MaintenanceTargetCode`).
    Target,
    /// Most recent cheap or detailed sample instant.
    LastSampledAt,
    /// Physical relation bytes in the most recent sample.
    LastRelationBytes,
    /// Estimated reclaimable bytes in the most recent sample.
    LastReclaimableBytes,
    /// Measured degradation percentage in the most recent sample.
    LastDegradedPercent,
    /// Profile threshold compared by the most recent sample.
    LastThresholdPercent,
    /// Policy decision for the most recent evaluation (`MaintenanceDecisionCode`).
    LastDecision,
    /// First sample of the unbroken run meeting every degradation threshold,
    /// NULL when no run is open.
    FirstBreachAt,
    /// Samples in the open breach run, zero when none is open.
    BreachSamples,
    /// Whether a later pass should execute the qualified operation.
    IsActionPending,
    /// Successful-action cooldown deadline.
    CooldownUntil,
    /// Consecutive action failures.
    ConsecutiveFailures,
    /// Most recent action attempt instant.
    LastActionAt,
    /// Most recent action result (`MaintenanceActionResultCode`).
    LastActionResult,
    /// Physical bytes immediately before the last successful rebuild, NULL
    /// for a heap target.
    LastBeforeBytes,
    /// Physical bytes immediately after the last successful rebuild, NULL
    /// for a heap target.
    LastAfterBytes,
    /// Dead tuples immediately before the last successful vacuum, NULL for an
    /// index target.
    LastDeadTuplesBefore,
    /// Dead tuples immediately after the last successful vacuum, NULL for an
    /// index target.
    LastDeadTuplesAfter,
}

#[cfg(test)]
mod tests {
    use crate::maintenance::{PostgresqlMaintenanceTargets, PostgresqlMaintenanceTenants};
    use pretty_assertions::assert_eq;
    use sea_query::Iden;

    #[test]
    fn platform_maintenance_identifiers_match_the_migration() {
        assert_eq!(PostgresqlMaintenanceTenants::Table.to_string(), "postgresql_maintenance_tenants");
        assert_eq!(PostgresqlMaintenanceTenants::LastInspectedAt.to_string(), "last_inspected_at");
        assert_eq!(PostgresqlMaintenanceTargets::Table.to_string(), "postgresql_maintenance_targets");
        assert_eq!(PostgresqlMaintenanceTargets::IsActionPending.to_string(), "is_action_pending");
        assert_eq!(PostgresqlMaintenanceTargets::LastReclaimableBytes.to_string(), "last_reclaimable_bytes");
        assert_eq!(PostgresqlMaintenanceTargets::LastBeforeBytes.to_string(), "last_before_bytes");
        assert_eq!(PostgresqlMaintenanceTargets::LastDeadTuplesBefore.to_string(), "last_dead_tuples_before");
        assert_eq!(PostgresqlMaintenanceTargets::LastDeadTuplesAfter.to_string(), "last_dead_tuples_after");
        assert_eq!(PostgresqlMaintenanceTargets::FirstBreachAt.to_string(), "first_breach_at");
        assert_eq!(PostgresqlMaintenanceTargets::BreachSamples.to_string(), "breach_samples");
    }
}
