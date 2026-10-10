//! Bounded-cardinality PostgreSQL maintenance metrics.

use crate::{
    names::{
        LABEL_ACTION,
        LABEL_DECISION,
        LABEL_OUTCOME,
        POSTGRESQL_MAINTENANCE_ACTION_SECONDS,
        POSTGRESQL_MAINTENANCE_ACTIONS_TOTAL,
        POSTGRESQL_MAINTENANCE_DECISIONS_TOTAL,
        POSTGRESQL_MAINTENANCE_RECLAIMED_BYTES,
        POSTGRESQL_MAINTENANCE_REMOVED_DEAD_TUPLES,
    },
    sample_value::SampleValue,
};
use metrics::{counter, histogram};
use std::time::Duration;
use strum::IntoStaticStr;

/// Stable maintenance pass decision labels.
#[derive(Clone, Copy, Debug, IntoStaticStr, Eq, PartialEq)]
#[strum(serialize_all = "kebab-case")]
pub enum MaintenanceDecision {
    /// Maintenance is disabled.
    Disabled,
    /// Another replica owns the fleet lock.
    GlobalLockUnavailable,
    /// No tenant is registered.
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
        maintenance::{ActionEffect, MaintenanceAction, MaintenanceActionOutcome, MaintenanceDecision, record_action, record_decision},
        names::{
            POSTGRESQL_MAINTENANCE_ACTIONS_TOTAL,
            POSTGRESQL_MAINTENANCE_DECISIONS_TOTAL,
            POSTGRESQL_MAINTENANCE_RECLAIMED_BYTES,
            POSTGRESQL_MAINTENANCE_REMOVED_DEAD_TUPLES,
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
}
