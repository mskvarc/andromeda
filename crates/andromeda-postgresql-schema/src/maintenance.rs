//! Platform-database relations that retain PostgreSQL maintenance state.

use sea_query::Iden;

/// Per-tenant inspection ordering and failure state.
#[derive(Clone, Copy, Iden)]
pub enum PostgresqlMaintenanceTenants {
    /// Table name: `postgresql_maintenance_tenants`.
    Table,
    /// Foreign key to the platform tenant registry.
    TenantId,
    /// Most recent bounded inspection pass.
    LastInspectedAt,
    /// Consecutive database-reachability failures.
    ConsecutiveFailures,
    /// Stable last-pass outcome label.
    LastResult,
}

/// Per-target degradation evidence, pending work and action results.
#[derive(Clone, Copy, Iden)]
pub enum PostgresqlMaintenanceTargets {
    /// Table name: `postgresql_maintenance_targets`.
    Table,
    /// Foreign key to the platform tenant registry.
    TenantId,
    /// Closed-set relation key.
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
    /// Stable policy decision for the most recent evaluation.
    LastDecision,
    /// Consecutive samples meeting every degradation threshold.
    ConsecutiveBreaches,
    /// Whether a later pass should execute the qualified operation.
    PendingAction,
    /// Successful-action cooldown deadline.
    CooldownUntil,
    /// Consecutive action failures.
    ConsecutiveFailures,
    /// Most recent action attempt instant.
    LastActionAt,
    /// Stable most recent action result label.
    LastActionResult,
    /// Physical bytes immediately before the last successful action.
    LastBeforeBytes,
    /// Physical bytes immediately after the last successful action.
    LastAfterBytes,
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
        assert_eq!(PostgresqlMaintenanceTargets::PendingAction.to_string(), "pending_action");
        assert_eq!(PostgresqlMaintenanceTargets::LastReclaimableBytes.to_string(), "last_reclaimable_bytes");
        assert_eq!(PostgresqlMaintenanceTargets::LastBeforeBytes.to_string(), "last_before_bytes");
    }
}
