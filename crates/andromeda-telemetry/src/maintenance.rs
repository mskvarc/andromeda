//! Bounded-cardinality PostgreSQL maintenance metrics.

use crate::{
    names::{
        LABEL_ACTION,
        LABEL_DECISION,
        LABEL_LEVEL,
        LABEL_OUTCOME,
        POSTGRESQL_AUTOVACUUM_WORKERS_BUSY,
        POSTGRESQL_AUTOVACUUM_WORKERS_MAX,
        POSTGRESQL_DATABASES_FREEZE_PRESSURE,
        POSTGRESQL_FREEZE_AGE_MAX,
        POSTGRESQL_MAINTENANCE_ACTION_SECONDS,
        POSTGRESQL_MAINTENANCE_ACTIONS_TOTAL,
        POSTGRESQL_MAINTENANCE_DECISIONS_TOTAL,
        POSTGRESQL_MAINTENANCE_RECLAIMED_BYTES,
        POSTGRESQL_MAINTENANCE_REMOVED_DEAD_TUPLES,
        POSTGRESQL_MULTIXACT_AGE_MAX,
        POSTGRESQL_OVERDUE_RELATIONS,
    },
    sample_value::SampleValue,
};
use metrics::{counter, gauge, histogram};
use std::time::Duration;
use strum::{EnumIter, IntoEnumIterator, IntoStaticStr};

/// Stable maintenance pass decision labels.
#[derive(Clone, Copy, Debug, IntoStaticStr, Eq, PartialEq)]
#[strum(serialize_all = "kebab-case")]
pub enum MaintenanceDecision {
    /// Maintenance is disabled.
    Disabled,
    /// Another replica owns the fleet lock.
    GlobalLockUnavailable,
    /// No tenant is registered, or none is due a visit.
    NoTenant,
    /// Tenant database could not be reached.
    TenantUnavailable,
    /// Tenant schema is behind the running contract.
    SchemaBehind,
    /// Required extension or privileges are absent.
    CapabilityUnavailable,
    /// No suspicious target was found.
    Idle,
    /// One detailed physical probe completed.
    Probed,
    /// Work was qualified for a later pass.
    Queued,
    /// Observe mode reported work without mutation.
    WouldAct,
    /// A safety gate deferred work.
    Deferred,
    /// One action completed.
    Acted,
    /// One action failed.
    ActionFailed,
    /// Cooperative cancellation stopped the pass.
    Cancelled,
}

/// Stable maintenance action-family labels.
#[derive(Clone, Copy, Debug, IntoStaticStr, Eq, PartialEq)]
#[strum(serialize_all = "kebab-case")]
pub enum MaintenanceAction {
    /// One index concurrent rebuild.
    Reindex,
    /// One ordinary catch-up vacuum.
    Vacuum,
    /// One catch-up `ANALYZE` of a heap's planner statistics.
    Analyze,
    /// One documented invalid-index artifact drop.
    Recovery,
}

/// Stable maintenance action outcomes.
#[derive(Clone, Copy, Debug, IntoStaticStr, Eq, PartialEq)]
#[strum(serialize_all = "kebab-case")]
pub enum MaintenanceActionOutcome {
    /// Statement completed successfully.
    Succeeded,
    /// Statement returned an error.
    Failed,
    /// Cooperative cancellation stopped the statement future.
    Cancelled,
}

/// What one successful maintenance action measurably achieved.
///
/// A rebuild writes a new index file, so its effect is the physical bytes it
/// gave back. A vacuum without truncation never shrinks its heap's file, so
/// its effect is the dead tuples it removed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActionEffect {
    /// Physical bytes a rebuild returned, zero when the relation did not shrink.
    ReclaimedBytes(u64),
    /// Dead tuples a vacuum removed, zero when the dead-tuple count did not fall.
    RemovedDeadTuples(u64),
}

/// Stable database freeze pressure labels.
#[derive(Clone, Copy, Debug, EnumIter, IntoStaticStr, Eq, PartialEq)]
#[strum(serialize_all = "kebab-case")]
pub enum FreezePressureLevel {
    /// Anti-wraparound vacuum is within its window.
    Normal,
    /// Anti-wraparound vacuum is overdue.
    Overdue,
    /// About half of the wraparound limit is spent.
    Critical,
}

/// Record one observation of the tenant fleet's freeze ages: the oldest
/// transaction ID and multixact ages, and how many databases stand at each
/// pressure level. Every level is set, so one that empties reads zero.
pub fn record_fleet_freeze(transaction_age_max: u64, multixact_age_max: u64, levels: &[FreezePressureLevel]) {
    gauge!(POSTGRESQL_FREEZE_AGE_MAX).set(SampleValue::from(transaction_age_max));
    gauge!(POSTGRESQL_MULTIXACT_AGE_MAX).set(SampleValue::from(multixact_age_max));
    for level in FreezePressureLevel::iter() {
        let databases = levels.iter().filter(|observed| **observed == level).count();
        let level: &'static str = level.into();
        gauge!(POSTGRESQL_DATABASES_FREEZE_PRESSURE, LABEL_LEVEL => level).set(SampleValue::from(databases));
    }
}

/// Record one observation of the cluster's autovacuum workers: how many are
/// running and how many `autovacuum_max_workers` allows.
pub fn record_autovacuum_workers(busy: u64, max: u64) {
    gauge!(POSTGRESQL_AUTOVACUUM_WORKERS_BUSY).set(SampleValue::from(busy));
    gauge!(POSTGRESQL_AUTOVACUUM_WORKERS_MAX).set(SampleValue::from(max));
}

/// Record how many relations one tenant visit found overdue for autovacuum,
/// zero included, without naming the tenant.
pub fn record_overdue_relations(relations: usize) {
    gauge!(POSTGRESQL_OVERDUE_RELATIONS).set(SampleValue::from(relations));
}

/// Count one bounded pass decision without a tenant label.
pub fn record_decision(decision: MaintenanceDecision) {
    let decision: &'static str = decision.into();
    counter!(POSTGRESQL_MAINTENANCE_DECISIONS_TOTAL, LABEL_DECISION => decision).increment(1);
}

/// Record one action outcome and duration, and the effect of a successful
/// action. An action that failed or was cancelled has no effect.
pub fn record_action(action: MaintenanceAction, outcome: MaintenanceActionOutcome, duration: Duration, effect: Option<ActionEffect>) {
    let action: &'static str = action.into();
    let outcome: &'static str = outcome.into();
    counter!(POSTGRESQL_MAINTENANCE_ACTIONS_TOTAL, LABEL_ACTION => action, LABEL_OUTCOME => outcome).increment(1);
    histogram!(POSTGRESQL_MAINTENANCE_ACTION_SECONDS, LABEL_ACTION => action, LABEL_OUTCOME => outcome).record(duration);
    match effect {
        Some(ActionEffect::ReclaimedBytes(bytes)) => {
            histogram!(POSTGRESQL_MAINTENANCE_RECLAIMED_BYTES, LABEL_ACTION => action).record(SampleValue::from(bytes));
        }
        Some(ActionEffect::RemovedDeadTuples(tuples)) => {
            histogram!(POSTGRESQL_MAINTENANCE_REMOVED_DEAD_TUPLES, LABEL_ACTION => action).record(SampleValue::from(tuples));
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::TelemetryError,
        maintenance::{
            ActionEffect,
            FreezePressureLevel,
            MaintenanceAction,
            MaintenanceActionOutcome,
            MaintenanceDecision,
            record_action,
            record_autovacuum_workers,
            record_decision,
            record_fleet_freeze,
            record_overdue_relations,
        },
        names::{
            POSTGRESQL_AUTOVACUUM_WORKERS_BUSY,
            POSTGRESQL_AUTOVACUUM_WORKERS_MAX,
            POSTGRESQL_DATABASES_FREEZE_PRESSURE,
            POSTGRESQL_FREEZE_AGE_MAX,
            POSTGRESQL_MAINTENANCE_ACTIONS_TOTAL,
            POSTGRESQL_MAINTENANCE_DECISIONS_TOTAL,
            POSTGRESQL_MAINTENANCE_RECLAIMED_BYTES,
            POSTGRESQL_MAINTENANCE_REMOVED_DEAD_TUPLES,
            POSTGRESQL_MULTIXACT_AGE_MAX,
            POSTGRESQL_OVERDUE_RELATIONS,
        },
        recorder::TelemetryRecorder,
    };
    use std::time::Duration;

    #[test]
    fn maintenance_metrics_have_no_tenant_dimension() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| {
            record_decision(MaintenanceDecision::Deferred);
            record_action(
                MaintenanceAction::Reindex,
                MaintenanceActionOutcome::Succeeded,
                Duration::from_secs(1),
                Some(ActionEffect::ReclaimedBytes(1024)),
            );
        });
        let rendered = recorder.render();
        assert!(rendered.contains(POSTGRESQL_MAINTENANCE_DECISIONS_TOTAL), "{rendered}");
        assert!(rendered.contains(POSTGRESQL_MAINTENANCE_ACTIONS_TOTAL), "{rendered}");
        assert!(!rendered.contains("tenant="), "{rendered}");
        Ok(())
    }

    #[test]
    fn each_action_effect_is_recorded_in_its_own_histogram() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| {
            record_action(
                MaintenanceAction::Vacuum,
                MaintenanceActionOutcome::Succeeded,
                Duration::from_secs(1),
                Some(ActionEffect::RemovedDeadTuples(49_000)),
            );
        });
        let rendered = recorder.render();
        assert!(rendered.contains(POSTGRESQL_MAINTENANCE_REMOVED_DEAD_TUPLES), "{rendered}");
        assert!(!rendered.contains(POSTGRESQL_MAINTENANCE_RECLAIMED_BYTES), "{rendered}");
        Ok(())
    }

    #[test]
    fn fleet_freeze_gauges_count_every_level_without_a_tenant_dimension() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| {
            record_fleet_freeze(
                260_000_000,
                1_000,
                &[FreezePressureLevel::Normal, FreezePressureLevel::Overdue, FreezePressureLevel::Normal],
            );
        });
        let rendered = recorder.render();
        assert!(rendered.contains(&format!("{POSTGRESQL_FREEZE_AGE_MAX} 260000000")), "{rendered}");
        assert!(rendered.contains(&format!("{POSTGRESQL_MULTIXACT_AGE_MAX} 1000")), "{rendered}");
        for (level, databases) in [("normal", 2), ("overdue", 1), ("critical", 0)] {
            assert!(
                rendered.contains(&format!("{POSTGRESQL_DATABASES_FREEZE_PRESSURE}{{level=\"{level}\"}} {databases}")),
                "{rendered}"
            );
        }
        assert!(!rendered.contains("tenant="), "{rendered}");
        Ok(())
    }

    #[test]
    fn autovacuum_health_gauges_have_no_tenant_dimension() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| {
            record_autovacuum_workers(3, 3);
            record_overdue_relations(2);
        });
        let rendered = recorder.render();
        assert!(rendered.contains(&format!("{POSTGRESQL_AUTOVACUUM_WORKERS_BUSY} 3")), "{rendered}");
        assert!(rendered.contains(&format!("{POSTGRESQL_AUTOVACUUM_WORKERS_MAX} 3")), "{rendered}");
        assert!(rendered.contains(&format!("{POSTGRESQL_OVERDUE_RELATIONS} 2")), "{rendered}");
        assert!(!rendered.contains("tenant="), "{rendered}");
        Ok(())
    }
}
