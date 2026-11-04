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
    /// The database's `tup_inserted + tup_updated + tup_deleted` when its
    /// last visit started, NULL before a visit observed the database.
    LastTupleChanges,
    /// The database's `stats_reset` when its last visit started, NULL before
    /// a visit observed it or while its statistics were never reset.
    LastStatsReset,
}

/// Per-target degradation evidence, pending work and action results.
#[derive(Clone, Copy, Iden)]
pub enum PostgresqlMaintenanceTargets {
    /// Table name: `postgresql_maintenance_targets`.
    Table,
    /// Foreign key to the platform tenant registry.
    TenantKey,
    /// The Snapshot whose published branch schema holds the relation, NULL for
    /// the live branch.
    SnapshotKey,
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
    /// A density-measured GIN's keys per covered row in thousandths of a key,
    /// sampled from its table whenever `analyzed_rows` is recorded; NULL for
    /// every other target and until a visit samples it.
    KeysPerRow,
    /// A density-measured GIN's bytes per key in thousandths of a byte,
    /// measured after each successful rebuild, or on its first observation for
    /// a GIN of a shape with no default; NULL for every other target and until
    /// then.
    KeyBytes,
    /// The row count a heap or a density-measured GIN is judged by, as its
    /// table's latest analyze left it; NULL for every other target and until a
    /// visit reads the target after an analyze.
    AnalyzedRows,
    /// The instant of the analyze `analyzed_rows` comes from, NULL exactly when
    /// it is.
    RowsAnalyzedAt,
}

/// The one row of maintenance state that belongs to the whole cluster.
#[derive(Clone, Copy, Iden)]
pub enum PostgresqlMaintenanceFleet {
    /// Table name: `postgresql_maintenance_fleet`.
    Table,
    /// Always `true`: the primary key that keeps the table to one row.
    Singleton,
    /// Consecutive passes that found every autovacuum worker slot busy, zero
    /// once a pass finds one free.
    AutovacuumSaturatedPasses,
}

#[cfg(test)]
mod tests {
    use crate::maintenance::{PostgresqlMaintenanceFleet, PostgresqlMaintenanceTargets, PostgresqlMaintenanceTenants};
    use pretty_assertions::assert_eq;
    use sea_query::Iden;

    #[test]
    fn platform_maintenance_identifiers_match_the_migration() {
        assert_eq!(PostgresqlMaintenanceTenants::Table.to_string(), "postgresql_maintenance_tenants");
        assert_eq!(PostgresqlMaintenanceTenants::LastInspectedAt.to_string(), "last_inspected_at");
        assert_eq!(PostgresqlMaintenanceTenants::LastTupleChanges.to_string(), "last_tuple_changes");
        assert_eq!(PostgresqlMaintenanceTenants::LastStatsReset.to_string(), "last_stats_reset");
        assert_eq!(PostgresqlMaintenanceTargets::Table.to_string(), "postgresql_maintenance_targets");
        assert_eq!(PostgresqlMaintenanceTargets::IsActionPending.to_string(), "is_action_pending");
        assert_eq!(PostgresqlMaintenanceTargets::LastReclaimableBytes.to_string(), "last_reclaimable_bytes");
        assert_eq!(PostgresqlMaintenanceTargets::LastBeforeBytes.to_string(), "last_before_bytes");
        assert_eq!(PostgresqlMaintenanceTargets::LastDeadTuplesBefore.to_string(), "last_dead_tuples_before");
        assert_eq!(PostgresqlMaintenanceTargets::LastDeadTuplesAfter.to_string(), "last_dead_tuples_after");
        assert_eq!(PostgresqlMaintenanceTargets::FirstBreachAt.to_string(), "first_breach_at");
        assert_eq!(PostgresqlMaintenanceTargets::BreachSamples.to_string(), "breach_samples");
        assert_eq!(PostgresqlMaintenanceTargets::KeysPerRow.to_string(), "keys_per_row");
        assert_eq!(PostgresqlMaintenanceTargets::KeyBytes.to_string(), "key_bytes");
        assert_eq!(PostgresqlMaintenanceTargets::AnalyzedRows.to_string(), "analyzed_rows");
        assert_eq!(PostgresqlMaintenanceTargets::RowsAnalyzedAt.to_string(), "rows_analyzed_at");
        assert_eq!(PostgresqlMaintenanceTargets::SnapshotKey.to_string(), "snapshot_key");
        assert_eq!(PostgresqlMaintenanceFleet::Table.to_string(), "postgresql_maintenance_fleet");
        assert_eq!(PostgresqlMaintenanceFleet::Singleton.to_string(), "singleton");
        assert_eq!(PostgresqlMaintenanceFleet::AutovacuumSaturatedPasses.to_string(), "autovacuum_saturated_passes");
    }
}
