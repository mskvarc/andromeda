//! Recovery of the repacks an interruption left, first in every visit.
//!
//! A repack's trigger logs every write of its table until the run's objects
//! are dropped, so objects nobody is running are cleaned up before anything
//! else the visit does. They are found in the tenant database's catalog, so a
//! run whose platform row is gone, or a run of the command line on an
//! allowlisted table, is recovered too, while a run that still holds its
//! table's lock is left alone. A recorded run with nothing left in the
//! database is forgotten once no repack runs anywhere in the fleet. A visit
//! recovers at most one table, like every other action.

use crate::{
    action_policy::{ActionEnding, cooldown_after},
    capability::{self, MaintainPrivilege},
    controller::PassOutcome,
    error::MaintenanceError,
    evidence::{action_result::ActionResult, inspection_result::InspectionResult},
    policy::PolicyProfile,
    repack::leftover::{self, CleanupOutcome, RepackLeftover},
    state::{FleetLock, FleetLockTaken, RegisteredTenant, fleet_lock_held_elsewhere, repack_run, target_record, tenant_inspection},
};
use andromeda_configuration::schema::database::maintenance::MaintenanceMode;
use andromeda_retry::retry_number::RetryNumber;
use chrono::Utc;
use sqlx::PgConnection;
use std::num::NonZeroU32;
use tracing::{info, warn};

/// Clean up the first leftover of the tenant nobody runs, and forget the
/// recorded runs with nothing left. Returns the outcome that ends the visit
/// when it recovered, or failed to recover, a table, `None` when the visit
/// goes on.
pub(crate) async fn recover_repacks(
    mode: MaintenanceMode,
    platform: &mut PgConnection,
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    profile: PolicyProfile,
) -> Result<Option<PassOutcome>, MaintenanceError> {
    let leftovers = leftover::read_leftovers(&mut *connection).await?;
    forget_finished_runs(platform, tenant, &leftovers).await?;
    for leftover in &leftovers {
        if let Some(outcome) = recover_leftover(mode, platform, &mut *connection, tenant, leftover, profile).await? {
            return Ok(Some(outcome));
        }
    }
    Ok(None)
}

/// Delete the recorded runs of the tenant whose table has nothing left,
/// while no repack runs in the fleet.
async fn forget_finished_runs(platform: &mut PgConnection, tenant: &RegisteredTenant, leftovers: &[RepackLeftover]) -> Result<(), MaintenanceError> {
    let runs = repack_run::tenant_runs(platform, tenant.key).await?;
    let finished = runs
        .into_iter()
        .filter(|run| !leftovers.iter().any(|leftover| leftover.relid == run.relid))
        .collect::<Vec<_>>();
    if finished.is_empty() || fleet_lock_held_elsewhere(platform, FleetLock::Repack).await? == FleetLockTaken::HeldElsewhere {
        return Ok(());
    }
    for run in finished {
        repack_run::record_end(platform, run.tenant, run.row).await?;
        info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?run.row.branch(), target = run.row.target().relation_name(), phase = %run.phase, started_at = %run.started_at, decision = "repack-run-forgotten", "PostgreSQL maintenance forgot a repack run that left nothing behind");
    }
    Ok(())
}

/// Clean up one leftover, `None` when the visit goes on: the table is not an
/// allowlisted heap, a run still holds it, the visit only observes, or the
/// role may not drop its objects.
async fn recover_leftover(
    mode: MaintenanceMode,
    platform: &mut PgConnection,
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    leftover: &RepackLeftover,
    profile: PolicyProfile,
) -> Result<Option<PassOutcome>, MaintenanceError> {
    let Some((schema, name)) = leftover::identity(&mut *connection, leftover.relid).await? else {
        return Ok(None);
    };
    let Some(row) = leftover::repack_target(&schema, &name) else {
        warn!(tenant = tenant.log_name(), database = %tenant.database, relation = %format!("{schema}.{name}"), objects = ?leftover.objects, decision = "repack-leftover-reported", "PostgreSQL repack objects on a table outside the allowlist require operator cleanup");
        return Ok(None);
    };
    if mode != MaintenanceMode::Automatic {
        info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?row.branch(), target = name.as_str(), objects = ?leftover.objects, decision = "would-recover-repack", "PostgreSQL maintenance recovery observed");
        return Ok(None);
    }
    if capability::ownership(&mut *connection, &format!("{schema}.{name}")).await? == MaintainPrivilege::NotGranted {
        warn!(tenant = tenant.log_name(), database = %tenant.database, branch = ?row.branch(), target = name.as_str(), decision = "capability-unavailable", reason = "missing-ownership", "PostgreSQL maintenance only observes repack objects the role may not drop");
        return Ok(None);
    }
    let cleanup = leftover::clean_up(&mut *connection, leftover.relid).await?;
    match cleanup {
        CleanupOutcome::HeldElsewhere => Ok(None),
        CleanupOutcome::Vanished => {
            repack_run::record_end(platform, tenant.key, row).await?;
            Ok(None)
        }
        CleanupOutcome::Cleaned => {
            repack_run::record_end(platform, tenant.key, row).await?;
            let state = target_record::load_target_state(platform, tenant, row).await?;
            let consecutive_failures = RetryNumber::new(NonZeroU32::MIN.saturating_add(state.consecutive_failures));
            let cooldown_until = cooldown_after(profile, row.target(), ActionEnding::Failed { consecutive_failures }, Utc::now());
            target_record::record_action_failure(platform, tenant, row, ActionResult::RepackRecovered, cooldown_until).await?;
            tenant_inspection::mark_inspected(platform, tenant, InspectionResult::Acted).await?;
            info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?row.branch(), target = name.as_str(), objects = ?leftover.objects, cooldown_until = %cooldown_until, decision = "repack-recovered", "PostgreSQL maintenance cleaned up an interrupted repack");
            Ok(Some(PassOutcome::Acted))
        }
        CleanupOutcome::LockWait => {
            tenant_inspection::mark_inspected(platform, tenant, InspectionResult::Deferred).await?;
            warn!(tenant = tenant.log_name(), database = %tenant.database, branch = ?row.branch(), target = name.as_str(), decision = "deferred", reason = "lock-wait", "PostgreSQL maintenance deferred the cleanup of an interrupted repack");
            Ok(Some(PassOutcome::Deferred))
        }
    }
}
