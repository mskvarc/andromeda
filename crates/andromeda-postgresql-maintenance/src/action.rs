//! Cancellation-aware execution and recovery for qualified maintenance work.

use crate::{
    action_measurement::{ActionBaseline, ActionComparison},
    action_policy::{ActionEnding, ActionVerdict, action_verdict, cooldown_after},
    artifact::{ArtifactOwner, RecoveryArtifact},
    branch::{Branch, BranchTarget},
    candidate::{MaintenanceTarget, heap_target::HeapTarget, history_table::HistoryTable, index_target::IndexTarget},
    controller::{PassOutcome, density_sampling},
    error::MaintenanceError,
    evidence::inspection_result::InspectionResult,
    gin_density::{self, Density, DensityReading},
    inspection,
    policy::{DeferralReason, DegradationSample, PolicyDecision, PolicyProfile, decide, safety_deferral},
    prepared_action::{Preparation, prepare_action, read_comparison},
    rowstore_rebuild,
    safety_gate_inspection::{self, GateScope},
    state::{RegisteredTenant, density_baseline, pending_action, target_record, tenant_inspection},
    vacuum_horizon::{self, HorizonHolder, HorizonScope},
};
use andromeda_configuration::schema::database::maintenance::MaintenanceMode;
use andromeda_postgresql_seaquery::{command::Command, maintenance::MaintenanceCommand};
use andromeda_retry::retry_number::RetryNumber;
use andromeda_telemetry::maintenance::{MaintenanceAction, MaintenanceActionOutcome, record_action};
use chrono::Utc;
use sqlx::PgConnection;
use std::{num::NonZeroU32, time::Instant};
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
    let branch = artifact.name.branch();
    let scope = match artifact.name.owner() {
        ArtifactOwner::Index { branch, index } => GateScope::Target(BranchTarget::new(*branch, MaintenanceTarget::Index(*index))?),
        ArtifactOwner::Chunk { table, chunk, .. } => GateScope::Chunk { table: *table, chunk },
    };
    let gates = safety_gate_inspection::safety_gates(tenant_connection, scope).await?;
    if let Some(reason) = safety_deferral(profile, gates) {
        tenant_inspection::mark_inspected(platform, tenant, InspectionResult::Deferred).await?;
        info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?branch, target = artifact.name.as_str(), artifact_kind = ?artifact.kind, decision = "deferred", reason = ?reason, "PostgreSQL maintenance recovery deferred");
        return Ok(PassOutcome::Deferred);
    }
    if mode != MaintenanceMode::Automatic {
        tenant_inspection::mark_inspected(platform, tenant, InspectionResult::WouldAct).await?;
        info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?branch, target = artifact.name.as_str(), artifact_kind = ?artifact.kind, decision = "would-drop-artifact", "PostgreSQL maintenance recovery observed");
        return Ok(PassOutcome::WouldAct);
    }
    let statement = artifact.name.drop_statement();
    let started = Instant::now();
    let result = execute_statement(tenant_connection, &statement, cancellation).await;
    match result {
        StatementOutcome::Succeeded => {
            record_action(MaintenanceAction::Recovery, MaintenanceActionOutcome::Succeeded, started.elapsed(), None);
            tenant_inspection::mark_inspected(platform, tenant, InspectionResult::Acted).await?;
            info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?branch, target = artifact.name.as_str(), artifact_kind = ?artifact.kind, decision = "dropped-artifact", "PostgreSQL concurrent-index recovery completed");
            Ok(PassOutcome::Acted)
        }
        StatementOutcome::Cancelled => {
            record_action(MaintenanceAction::Recovery, MaintenanceActionOutcome::Cancelled, started.elapsed(), None);
            Ok(PassOutcome::Cancelled)
        }
        StatementOutcome::Failed(error) => {
            record_action(MaintenanceAction::Recovery, MaintenanceActionOutcome::Failed, started.elapsed(), None);
            tenant_inspection::mark_inspected(platform, tenant, InspectionResult::ActionFailed).await?;
            warn!(tenant = tenant.log_name(), database = %tenant.database, branch = ?branch, target = artifact.name.as_str(), artifact_kind = ?artifact.kind, error = %error, decision = "artifact-drop-failed", "PostgreSQL concurrent-index recovery failed");
            Ok(PassOutcome::ActionFailed)
        }
    }
}

/// Execute one action qualified on an earlier pass, in the branch its target
/// row belongs to.
pub(crate) async fn execute_pending(
    mode: MaintenanceMode,
    platform: &mut PgConnection,
    tenant_connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    row: BranchTarget,
    profile: PolicyProfile,
    cancellation: &CancellationToken,
) -> Result<PassOutcome, MaintenanceError> {
    let (branch, target) = (row.branch(), row.target());
    let action = target_action(target);
    let prepared = match prepare_action(platform, tenant_connection, tenant, row, profile).await? {
        Preparation::Ready(prepared) => prepared,
        Preparation::Lapsed(reason) => return drop_lapsed(platform, tenant, row, reason).await,
    };
    let gates = prepared.gates;
    let target_state = target_record::load_target_state(platform, tenant, row).await?;
    let now = Utc::now();
    let decision = decide(mode, profile, target_state, empty_sample(), gates, now);
    target_record::record_decision(platform, tenant, row, None, decision, target_state.breach, now).await?;
    match decision {
        PolicyDecision::WouldAct => {
            tenant_inspection::mark_inspected(platform, tenant, InspectionResult::WouldAct).await?;
            info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?branch, target = target.relation_name(), relation = prepared.relation.as_str(), action = ?action, decision = "would-act", "PostgreSQL maintenance pending action observed");
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
            info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?branch, target = target.relation_name(), action = ?action, decision = "deferred", reason = ?reason, horizon_holder = ?gates.held_horizon, "PostgreSQL maintenance pending action deferred");
            return Ok(PassOutcome::Deferred);
        }
        PolicyDecision::Queued => {
            tenant_inspection::mark_inspected(platform, tenant, InspectionResult::Queued).await?;
            return Ok(PassOutcome::Queued);
        }
        PolicyDecision::Act => {}
    }
    let (baseline, statement) = (prepared.baseline, prepared.statement);
    let started = Instant::now();
    match execute_statement(tenant_connection, &statement, cancellation).await {
        StatementOutcome::Cancelled => {
            record_action(action, MaintenanceActionOutcome::Cancelled, started.elapsed(), None);
            Ok(PassOutcome::Cancelled)
        }
        StatementOutcome::Failed(error) => {
            record_action(action, MaintenanceActionOutcome::Failed, started.elapsed(), None);
            let consecutive_failures = RetryNumber::new(NonZeroU32::MIN.saturating_add(target_state.consecutive_failures));
            let cooldown_until = cooldown_after(profile, target, ActionEnding::Failed { consecutive_failures }, Utc::now());
            target_record::record_action_failure(platform, tenant, row, cooldown_until).await?;
            tenant_inspection::mark_inspected(platform, tenant, InspectionResult::ActionFailed).await?;
            warn!(tenant = tenant.log_name(), database = %tenant.database, branch = ?branch, target = target.relation_name(), action = ?action, error = %error, duration_seconds = started.elapsed().as_secs_f64(), consecutive_failures = consecutive_failures.get(), cooldown_until = %cooldown_until, decision = "action-failed", "PostgreSQL maintenance action failed");
            Ok(PassOutcome::ActionFailed)
        }
        StatementOutcome::Succeeded => {
            let duration = started.elapsed();
            let comparison = read_comparison(tenant_connection, branch, baseline).await?;
            record_action(action, MaintenanceActionOutcome::Succeeded, duration, comparison.effect());
            let verdict = action_verdict(profile, target, comparison);
            let cooldown_until = cooldown_after(profile, target, ActionEnding::Completed(verdict), Utc::now());
            target_record::record_action_completion(platform, tenant, row, verdict, cooldown_until, comparison).await?;
            tenant_inspection::mark_inspected(platform, tenant, InspectionResult::Acted).await?;
            match comparison {
                ActionComparison::Index { before_bytes, after_bytes } => {
                    info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?branch, target = target.relation_name(), before_bytes, after_bytes, reclaimed_bytes = before_bytes.saturating_sub(after_bytes), verdict = ?verdict, cooldown_until = %cooldown_until, duration_seconds = duration.as_secs_f64(), decision = "acted", "PostgreSQL maintenance action completed");
                    if let MaintenanceTarget::Index(index) = target
                        && index.density_column().is_some()
                    {
                        reset_density_baseline(platform, tenant_connection, tenant, branch, index).await?;
                    }
                }
                ActionComparison::Heap { before, after } => {
                    info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?branch, target = target.relation_name(), dead_tuples_before = before.dead, dead_tuples_after = after.dead, removed_dead_tuples = before.dead.saturating_sub(after.dead), live_tuples_after = after.live, dead_percent_after = ?comparison.post_vacuum_dead_percent(), verdict = ?verdict, cooldown_until = %cooldown_until, duration_seconds = duration.as_secs_f64(), decision = "acted", "PostgreSQL maintenance action completed");
                    if verdict == ActionVerdict::Ineffective {
                        report_horizon_holders(tenant_connection, tenant, target).await?;
                    }
                }
                ActionComparison::Statistics {
                    before_modified,
                    after_modified,
                } => {
                    info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?branch, target = target.relation_name(), action = ?action, modified_since_analyze_before = before_modified, modified_since_analyze_after = after_modified, cooldown_until = %cooldown_until, duration_seconds = duration.as_secs_f64(), decision = "acted", "PostgreSQL maintenance action completed");
                }
                ActionComparison::Columnstore { after } => {
                    info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?branch, target = target.relation_name(), action = ?action, chunk = prepared.relation.as_str(), partiality_after = ?after, verdict = ?verdict, cooldown_until = %cooldown_until, duration_seconds = duration.as_secs_f64(), decision = "acted", "PostgreSQL maintenance action completed");
                }
                ActionComparison::Rowstore { before_bytes, after_bytes } => {
                    if let ActionBaseline::Rowstore { table, range_start, .. } = baseline {
                        rowstore_rebuild::record_rebuilt(tenant_connection, table, range_start).await?;
                    }
                    info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?branch, target = target.relation_name(), action = ?action, chunk = prepared.relation.as_str(), before_bytes, after_bytes, reclaimed_bytes = before_bytes.saturating_sub(after_bytes), verdict = ?verdict, cooldown_until = %cooldown_until, duration_seconds = duration.as_secs_f64(), decision = "acted", "PostgreSQL maintenance action completed");
                }
            }
            Ok(PassOutcome::Acted)
        }
    }
}

/// Record a rebuilt density-measured GIN's density as its new baseline, read
/// the way a catalog sample reads it, over its analyzed covered rows. A GIN
/// whose covered rows the statistics no longer describe has its baseline
/// cleared, so its next observation records one.
async fn reset_density_baseline(
    platform: &mut PgConnection,
    tenant_connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    branch: Branch,
    index: IndexTarget,
) -> Result<(), MaintenanceError> {
    let sizes = inspection::index_sizes(tenant_connection, branch).await?;
    let coverage = density_sampling::settled_coverage(platform, tenant_connection, tenant, branch).await?;
    let baseline = gin_density::density_readings(&sizes, &coverage)
        .find(|reading| reading.target == index)
        .and_then(DensityReading::density);
    density_baseline::record_density_baseline(platform, tenant, branch, index, baseline).await?;
    info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?branch, target = index.relation_name(), baseline_millibytes_per_row = ?baseline.map(Density::millibytes_per_row), "PostgreSQL maintenance density baseline reset after rebuild");
    Ok(())
}

/// Log every holder of the vacuum horizon after a vacuum that could not
/// remove what it was queued for, so an operator can release the one that
/// held it.
async fn report_horizon_holders(connection: &mut PgConnection, tenant: &RegisteredTenant, target: MaintenanceTarget) -> Result<(), MaintenanceError> {
    let holders = vacuum_horizon::horizon_holders(connection, HorizonScope::TenantDatabase).await?;
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

/// Withdraw every action queued in `branch`, which the visit only observes,
/// logging each as one automatic mode would run.
pub(crate) async fn withdraw_observed(platform: &mut PgConnection, tenant: &RegisteredTenant, branch: Branch) -> Result<(), MaintenanceError> {
    for target in pending_action::withdraw_actions(platform, tenant, branch).await? {
        info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?branch, target = target.relation_name(), action = ?target_action(target), decision = "would-act", "PostgreSQL maintenance withdrew a queued action from a branch it only observes");
    }
    Ok(())
}

/// Drop a pending action whose live reading no longer meets the threshold
/// that queued it: nothing runs, and a later sample queues it again if it
/// qualifies again.
async fn drop_lapsed(
    platform: &mut PgConnection,
    tenant: &RegisteredTenant,
    row: BranchTarget,
    reason: DeferralReason,
) -> Result<PassOutcome, MaintenanceError> {
    let target = row.target();
    target_record::record_lapsed_action(platform, tenant, row, reason).await?;
    tenant_inspection::mark_inspected(platform, tenant, InspectionResult::Idle).await?;
    info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?row.branch(), target = target.relation_name(), action = ?target_action(target), reason = ?reason, decision = "no-longer-qualifies", "PostgreSQL maintenance pending action dropped");
    Ok(PassOutcome::Idle)
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

/// Execute a maintenance command, abandoning it when `cancellation` fires.
async fn execute_statement(connection: &mut PgConnection, statement: &MaintenanceCommand, cancellation: &CancellationToken) -> StatementOutcome {
    tokio::select! {
        () = cancellation.cancelled() => StatementOutcome::Cancelled,
        result = statement.to_query().execute(connection) => match result {
            Ok(_) => StatementOutcome::Succeeded,
            Err(error) => StatementOutcome::Failed(error),
        },
    }
}

/// Map a closed-set target to a bounded action-family metric label.
const fn target_action(target: MaintenanceTarget) -> MaintenanceAction {
    match target {
        MaintenanceTarget::Index(
            IndexTarget::PkCaGrain
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
            | IndexTarget::IdxCaValueLongTextTrgm
            | IndexTarget::IdxNotificationDeliveriesClaim
            | IndexTarget::UqNotificationDeliveriesEventSubscription
            | IndexTarget::IdxNotificationDeliveriesSubscription
            | IndexTarget::IdxNotificationDeliveriesSubscriptionClaim
            | IndexTarget::PkNotificationDeliveries
            | IndexTarget::IdxEntitiesIdTrgm
            | IndexTarget::PkTemporalAttributeStreams
            | IndexTarget::UqTemporalAttributeStreamsGrain
            | IndexTarget::IdxTemporalAttributeStreamsAttributeNameKeyEntityKey
            | IndexTarget::IdxTemporalAttributeStreamsValueExtent
            | IndexTarget::IdxTemporalAttributeStreamsAttributeNameKeyShapeMask,
        )
        | MaintenanceTarget::Rowstore(HistoryTable::AttributeHistory) => MaintenanceAction::Reindex,
        MaintenanceTarget::Heap(
            HeapTarget::CurrentAttributes
            | HeapTarget::Entities
            | HeapTarget::SubscriptionDeliveryState
            | HeapTarget::NotificationEvents
            | HeapTarget::NotificationDeliveries
            | HeapTarget::PendingSideEffects
            | HeapTarget::TemporalAppendIntents
            | HeapTarget::SnapshotNotificationOutbox
            | HeapTarget::EntitySearch
            | HeapTarget::TemporalAttributeStreams,
        ) => MaintenanceAction::Vacuum,
        MaintenanceTarget::Statistics(
            HeapTarget::CurrentAttributes
            | HeapTarget::Entities
            | HeapTarget::SubscriptionDeliveryState
            | HeapTarget::NotificationEvents
            | HeapTarget::NotificationDeliveries
            | HeapTarget::PendingSideEffects
            | HeapTarget::TemporalAppendIntents
            | HeapTarget::SnapshotNotificationOutbox
            | HeapTarget::EntitySearch
            | HeapTarget::TemporalAttributeStreams,
        ) => MaintenanceAction::Analyze,
        MaintenanceTarget::Columnstore(HistoryTable::AttributeHistory) => MaintenanceAction::Recompress,
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
