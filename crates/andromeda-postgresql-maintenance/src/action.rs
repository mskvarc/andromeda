//! Cancellation-aware execution and recovery for qualified maintenance work.

use crate::{
    action_measurement::{ActionBaseline, ActionComparison},
    artifact::RecoveryArtifact,
    branch::{Branch, BranchTarget},
    candidate::{HeapTarget, IndexTarget, MaintenanceTarget},
    controller::PassOutcome,
    error::MaintenanceError,
    evidence::inspection_result::InspectionResult,
    gin_density::{self, Density, DensityReading},
    inspection,
    policy::{
        ActionEnding,
        ActionVerdict,
        DeferralReason,
        DegradationSample,
        PolicyDecision,
        PolicyProfile,
        action_verdict,
        cooldown_after,
        decide,
        safety_deferral,
    },
    safety_gate_inspection,
    state::{RegisteredTenant, density_baseline, target_record, tenant_inspection},
    vacuum_horizon::{self, HorizonHolder},
};
use andromeda_postgresql_context_store::maintenance_config::MaintenanceMode;
use andromeda_postgresql_seaquery::maintenance::MaintenanceStatement;
use andromeda_telemetry::maintenance::{MaintenanceAction, MaintenanceActionOutcome, record_action};
use chrono::Utc;
use sqlx::{Executor, PgConnection};
use std::time::Instant;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

/// Apply PostgreSQL's documented `_ccnew`/`_ccold` recovery procedure.
pub(crate) async fn recover_artifact(
    mode: MaintenanceMode,
    platform: &mut PgConnection,
    tenant_connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    artifact: RecoveryArtifact,
    profile: PolicyProfile,
    cancellation: &CancellationToken,
) -> Result<PassOutcome, MaintenanceError> {
    let target = MaintenanceTarget::Index(artifact.name.owner());
    let gates = safety_gate_inspection::safety_gates(tenant_connection, target).await?;
    if let Some(reason) = safety_deferral(profile, gates) {
        tenant_inspection::mark_inspected(platform, tenant, InspectionResult::Deferred).await?;
        info!(tenant = tenant.log_name(), database = %tenant.database, target = artifact.name.as_str(), artifact_kind = ?artifact.kind, decision = "deferred", reason = ?reason, "PostgreSQL maintenance recovery deferred");
        return Ok(PassOutcome::Deferred);
    }
    if mode != MaintenanceMode::Automatic {
        tenant_inspection::mark_inspected(platform, tenant, InspectionResult::WouldAct).await?;
        info!(tenant = tenant.log_name(), database = %tenant.database, target = artifact.name.as_str(), artifact_kind = ?artifact.kind, decision = "would-drop-artifact", "PostgreSQL maintenance recovery observed");
        return Ok(PassOutcome::WouldAct);
    }
    let statement = artifact.name.drop_statement();
    let started = Instant::now();
    let result = execute_statement(tenant_connection, &statement, cancellation).await;
    match result {
        StatementOutcome::Succeeded => {
            record_action(MaintenanceAction::Recovery, MaintenanceActionOutcome::Succeeded, started.elapsed(), None);
            tenant_inspection::mark_inspected(platform, tenant, InspectionResult::Acted).await?;
            info!(tenant = tenant.log_name(), database = %tenant.database, target = artifact.name.as_str(), artifact_kind = ?artifact.kind, decision = "dropped-artifact", "PostgreSQL concurrent-index recovery completed");
            Ok(PassOutcome::Acted)
        }
        StatementOutcome::Cancelled => {
            record_action(MaintenanceAction::Recovery, MaintenanceActionOutcome::Cancelled, started.elapsed(), None);
            Ok(PassOutcome::Cancelled)
        }
        StatementOutcome::Failed(error) => {
            record_action(MaintenanceAction::Recovery, MaintenanceActionOutcome::Failed, started.elapsed(), None);
            tenant_inspection::mark_inspected(platform, tenant, InspectionResult::ActionFailed).await?;
            warn!(tenant = tenant.log_name(), database = %tenant.database, target = artifact.name.as_str(), artifact_kind = ?artifact.kind, error = %error, decision = "artifact-drop-failed", "PostgreSQL concurrent-index recovery failed");
            Ok(PassOutcome::ActionFailed)
        }
    }
}

/// Execute one action of the live branch qualified on an earlier pass.
pub(crate) async fn execute_pending(
    mode: MaintenanceMode,
    platform: &mut PgConnection,
    tenant_connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    target: MaintenanceTarget,
    profile: PolicyProfile,
    cancellation: &CancellationToken,
) -> Result<PassOutcome, MaintenanceError> {
    let gates = safety_gate_inspection::safety_gates(tenant_connection, target).await?;
    let row = BranchTarget::live(target);
    let target_state = target_record::load_target_state(platform, tenant, row).await?;
    let now = Utc::now();
    let decision = decide(mode, profile, target_state, empty_sample(), gates, now);
    target_record::record_decision(platform, tenant, row, None, decision, target_state.breach, now).await?;
    match decision {
        PolicyDecision::WouldAct => {
            tenant_inspection::mark_inspected(platform, tenant, InspectionResult::WouldAct).await?;
            info!(tenant = tenant.log_name(), database = %tenant.database, target = target.relation_name(), decision = "would-act", "PostgreSQL maintenance pending action observed");
            return Ok(PassOutcome::WouldAct);
        }
        PolicyDecision::Deferred(
            reason @ (DeferralReason::RelationTooSmall
            | DeferralReason::ReclaimTooSmall
            | DeferralReason::BelowThreshold
            | DeferralReason::ConsecutiveSamples
            | DeferralReason::Cooldown
            | DeferralReason::ActiveBackendRatio
            | DeferralReason::LongTransaction
            | DeferralReason::HorizonHeld
            | DeferralReason::ActiveMaintenance
            | DeferralReason::WaitingRelationLock),
        ) => {
            tenant_inspection::mark_inspected(platform, tenant, InspectionResult::Deferred).await?;
            info!(tenant = tenant.log_name(), database = %tenant.database, target = target.relation_name(), decision = "deferred", reason = ?reason, horizon_holder = ?gates.held_horizon, "PostgreSQL maintenance pending action deferred");
            return Ok(PassOutcome::Deferred);
        }
        PolicyDecision::Queued => {
            tenant_inspection::mark_inspected(platform, tenant, InspectionResult::Queued).await?;
            return Ok(PassOutcome::Queued);
        }
        PolicyDecision::Act => {}
    }
    let baseline = read_baseline(tenant_connection, target).await?;
    let statement = target_statement(target);
    let action = target_action(target);
    let started = Instant::now();
    match execute_statement(tenant_connection, &statement, cancellation).await {
        StatementOutcome::Cancelled => {
            record_action(action, MaintenanceActionOutcome::Cancelled, started.elapsed(), None);
            Ok(PassOutcome::Cancelled)
        }
        StatementOutcome::Failed(error) => {
            record_action(action, MaintenanceActionOutcome::Failed, started.elapsed(), None);
            let consecutive_failures = target_state.consecutive_failures.saturating_add(1);
            let cooldown_until = cooldown_after(profile, ActionEnding::Failed { consecutive_failures }, Utc::now());
            target_record::record_action_failure(platform, tenant, row, cooldown_until).await?;
            tenant_inspection::mark_inspected(platform, tenant, InspectionResult::ActionFailed).await?;
            warn!(tenant = tenant.log_name(), database = %tenant.database, target = target.relation_name(), error = %error, duration_seconds = started.elapsed().as_secs_f64(), consecutive_failures, cooldown_until = %cooldown_until, decision = "action-failed", "PostgreSQL maintenance action failed");
            Ok(PassOutcome::ActionFailed)
        }
        StatementOutcome::Succeeded => {
            let duration = started.elapsed();
            let comparison = read_comparison(tenant_connection, baseline).await?;
            record_action(action, MaintenanceActionOutcome::Succeeded, duration, Some(comparison.effect()));
            let verdict = action_verdict(profile, target, comparison);
            let cooldown_until = cooldown_after(profile, ActionEnding::Completed(verdict), Utc::now());
            target_record::record_action_completion(platform, tenant, row, verdict, cooldown_until, comparison).await?;
            tenant_inspection::mark_inspected(platform, tenant, InspectionResult::Acted).await?;
            match comparison {
                ActionComparison::Index { before_bytes, after_bytes } => {
                    info!(tenant = tenant.log_name(), database = %tenant.database, target = target.relation_name(), before_bytes, after_bytes, reclaimed_bytes = before_bytes.saturating_sub(after_bytes), verdict = ?verdict, cooldown_until = %cooldown_until, duration_seconds = duration.as_secs_f64(), decision = "acted", "PostgreSQL maintenance action completed");
                    if let MaintenanceTarget::Index(index) = target
                        && index.density_column().is_some()
                    {
                        reset_density_baseline(platform, tenant_connection, tenant, index).await?;
                    }
                }
                ActionComparison::Heap { before, after } => {
                    info!(tenant = tenant.log_name(), database = %tenant.database, target = target.relation_name(), dead_tuples_before = before.dead, dead_tuples_after = after.dead, removed_dead_tuples = before.dead.saturating_sub(after.dead), live_tuples_after = after.live, dead_percent_after = ?comparison.post_vacuum_dead_percent(), verdict = ?verdict, cooldown_until = %cooldown_until, duration_seconds = duration.as_secs_f64(), decision = "acted", "PostgreSQL maintenance action completed");
                    if verdict == ActionVerdict::Ineffective {
                        report_horizon_holders(tenant_connection, tenant, target).await?;
                    }
                }
            }
            Ok(PassOutcome::Acted)
        }
    }
}

/// Record a rebuilt density-measured GIN's density as its new baseline, read
/// the way a catalog sample reads it. A GIN whose covered rows the statistics
/// no longer describe has its baseline cleared, so its next observation
/// records one.
async fn reset_density_baseline(
    platform: &mut PgConnection,
    tenant_connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    index: IndexTarget,
) -> Result<(), MaintenanceError> {
    let sizes = inspection::index_sizes(tenant_connection).await?;
    let coverage = gin_density::gin_coverage(tenant_connection).await?;
    let baseline = gin_density::density_readings(&sizes, &coverage)
        .find(|reading| reading.target == index)
        .and_then(DensityReading::density);
    density_baseline::record_density_baseline(platform, tenant, Branch::Live, index, baseline).await?;
    info!(tenant = tenant.log_name(), database = %tenant.database, target = index.relation_name(), baseline_millibytes_per_row = ?baseline.map(Density::millibytes_per_row), "PostgreSQL maintenance density baseline reset after rebuild");
    Ok(())
}

/// Log every holder of the vacuum horizon after a vacuum that could not
/// remove what it was queued for, so an operator can release the one that
/// held it.
async fn report_horizon_holders(connection: &mut PgConnection, tenant: &RegisteredTenant, target: MaintenanceTarget) -> Result<(), MaintenanceError> {
    let holders = vacuum_horizon::horizon_holders(connection).await?;
    if holders.is_empty() {
        warn!(tenant = tenant.log_name(), database = %tenant.database, target = target.relation_name(), decision = "ineffective", "PostgreSQL vacuum left dead tuples while no horizon holder remains");
    }
    for holder in holders {
        let kind: &'static str = holder.kind().into();
        match holder {
            HorizonHolder::ClientBackend { xmin_age, transaction_seconds } => {
                warn!(tenant = tenant.log_name(), database = %tenant.database, target = target.relation_name(), holder = kind, xmin_age, transaction_seconds = ?transaction_seconds, decision = "ineffective", "PostgreSQL vacuum horizon holder");
            }
            HorizonHolder::PreparedTransaction { xid_age, prepared_seconds } => {
                warn!(tenant = tenant.log_name(), database = %tenant.database, target = target.relation_name(), holder = kind, xmin_age = xid_age, prepared_seconds, decision = "ineffective", "PostgreSQL vacuum horizon holder");
            }
            HorizonHolder::ReplicationSlot {
                xmin_age,
                catalog_xmin_age,
                activity,
            } => {
                warn!(tenant = tenant.log_name(), database = %tenant.database, target = target.relation_name(), holder = kind, xmin_age = ?xmin_age, catalog_xmin_age = ?catalog_xmin_age, activity = ?activity, decision = "ineffective", "PostgreSQL vacuum horizon holder");
            }
            HorizonHolder::StandbyFeedback { xmin_age } => {
                warn!(tenant = tenant.log_name(), database = %tenant.database, target = target.relation_name(), holder = kind, xmin_age, decision = "ineffective", "PostgreSQL vacuum horizon holder");
            }
        }
    }
    Ok(())
}

/// Read the measure of a target's action before the action runs.
async fn read_baseline(connection: &mut PgConnection, target: MaintenanceTarget) -> Result<ActionBaseline, MaintenanceError> {
    Ok(match target {
        MaintenanceTarget::Index(index) => ActionBaseline::Index {
            index,
            before_bytes: inspection::index_bytes(connection, index).await?,
        },
        MaintenanceTarget::Heap(heap) => ActionBaseline::Heap {
            heap,
            before: inspection::heap_tuple_counts(connection, heap).await?,
        },
    })
}

/// Read the same measure after the action and pair it with the baseline.
async fn read_comparison(connection: &mut PgConnection, baseline: ActionBaseline) -> Result<ActionComparison, MaintenanceError> {
    Ok(match baseline {
        ActionBaseline::Index { index, before_bytes } => ActionComparison::Index {
            before_bytes,
            after_bytes: inspection::index_bytes(connection, index).await?,
        },
        ActionBaseline::Heap { heap, before } => ActionComparison::Heap {
            before,
            after: inspection::heap_tuple_counts(connection, heap).await?,
        },
    })
}

/// One maintenance statement's cancellation-aware result.
enum StatementOutcome {
    /// Statement completed successfully.
    Succeeded,
    /// Cooperative cancellation won the race and the connection will close.
    Cancelled,
    /// PostgreSQL rejected or interrupted the statement.
    Failed(sqlx::Error),
}

/// Execute a statement built by the PostgreSQL `SeaQuery` dialect extension.
async fn execute_statement(connection: &mut PgConnection, statement: &MaintenanceStatement, cancellation: &CancellationToken) -> StatementOutcome {
    tokio::select! {
        () = cancellation.cancelled() => StatementOutcome::Cancelled,
        result = connection.execute(sqlx::AssertSqlSafe(statement.as_sql())) => match result {
            Ok(_) => StatementOutcome::Succeeded,
            Err(error) => StatementOutcome::Failed(error),
        },
    }
}

/// Resolve the fixed statement belonging to one closed-set target.
fn target_statement(target: MaintenanceTarget) -> MaintenanceStatement {
    match target {
        MaintenanceTarget::Index(index) => index.reindex_statement(),
        MaintenanceTarget::Heap(heap) => heap.vacuum_statement(),
    }
}

/// Map a closed-set target to a bounded action-family metric label.
const fn target_action(target: MaintenanceTarget) -> MaintenanceAction {
    match target {
        MaintenanceTarget::Index(
            IndexTarget::UqCurrentAttributesGrain
            | IndexTarget::IdxCaValueNumber
            | IndexTarget::IdxCaValueString
            | IndexTarget::IdxCaValueStringTrgm
            | IndexTarget::IdxCaValueBoolean
            | IndexTarget::IdxCaValueDateTime
            | IndexTarget::IdxCaValueUri
            | IndexTarget::IdxCaValueGeometry
            | IndexTarget::IdxCaEntityGeometry
            | IndexTarget::IdxCaSubEqKeys
            | IndexTarget::IdxCaValueObject
            | IndexTarget::IdxCaValueLongTextTrgm,
        ) => MaintenanceAction::Reindex,
        MaintenanceTarget::Heap(
            HeapTarget::CurrentAttributes
            | HeapTarget::Entities
            | HeapTarget::SubscriptionDeliveryState
            | HeapTarget::NotificationEvents
            | HeapTarget::NotificationDeliveries
            | HeapTarget::PendingSideEffects
            | HeapTarget::TemporalAppendIntents
            | HeapTarget::SnapshotNotificationOutbox,
        ) => MaintenanceAction::Vacuum,
    }
}

/// Placeholder sample for an already-qualified pending operation.
const fn empty_sample() -> DegradationSample {
    DegradationSample {
        relation_bytes: 0,
        reclaimable_bytes: 0,
        degraded_percent: 0,
        threshold_percent: 0,
    }
}
