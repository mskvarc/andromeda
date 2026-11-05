//! Cancellation-aware execution and recovery for qualified maintenance work.

use crate::{
    action_completion::{self, Completion},
    action_policy::{ActionEnding, action_verdict, cooldown_after},
    artifact::{ArtifactOwner, RecoveryArtifact},
    branch::{Branch, BranchTarget},
    candidate::{MaintenanceTarget, heap_target::HeapTarget, history_table::HistoryTable, index_target::IndexTarget},
    controller::PassOutcome,
    error::MaintenanceError,
    evidence::{action_result::ActionResult, inspection_result::InspectionResult},
    policy::{DeferralReason, DegradationSample, PolicyDecision, PolicyProfile, decide, safety_deferral},
    prepared_action::{Preparation, prepare_action, read_comparison},
    safety_gate_inspection::{self, GateScope},
    state::{RegisteredTenant, pending_action, target_record, tenant_inspection},
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
            record_action(MaintenanceAction::Recovery, MaintenanceActionOutcome::Ok, started.elapsed(), None);
            tenant_inspection::mark_inspected(platform, tenant, InspectionResult::Acted).await?;
            info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?branch, target = artifact.name.as_str(), artifact_kind = ?artifact.kind, decision = "dropped-artifact", "PostgreSQL concurrent-index recovery completed");
            Ok(PassOutcome::Acted)
        }
        StatementOutcome::Cancelled => {
            record_action(MaintenanceAction::Recovery, MaintenanceActionOutcome::Cancelled, started.elapsed(), None);
            Ok(PassOutcome::Cancelled)
        }
        StatementOutcome::Failed(error) => {
            record_action(MaintenanceAction::Recovery, MaintenanceActionOutcome::Error, started.elapsed(), None);
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
        Preparation::OtherRunner => {
            tenant_inspection::mark_inspected(platform, tenant, InspectionResult::Idle).await?;
            return Ok(PassOutcome::Idle);
        }
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
            | DeferralReason::WaitingRelationLock
            | DeferralReason::VacuumPending
            | DeferralReason::RelationTooLarge
            | DeferralReason::ReplicationRetention
            | DeferralReason::SnapshotLifetimeShort
            | DeferralReason::RepackLeftover
            | DeferralReason::FreezePressure
            | DeferralReason::RepackRunning),
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
            record_action(action, MaintenanceActionOutcome::Error, started.elapsed(), None);
            let consecutive_failures = RetryNumber::new(NonZeroU32::MIN.saturating_add(target_state.consecutive_failures));
            let cooldown_until = cooldown_after(profile, target, ActionEnding::Failed { consecutive_failures }, Utc::now());
            target_record::record_action_failure(platform, tenant, row, ActionResult::Failed, cooldown_until).await?;
            tenant_inspection::mark_inspected(platform, tenant, InspectionResult::ActionFailed).await?;
            warn!(tenant = tenant.log_name(), database = %tenant.database, branch = ?branch, target = target.relation_name(), action = ?action, error = %error, duration_seconds = started.elapsed().as_secs_f64(), consecutive_failures = consecutive_failures.get(), cooldown_until = %cooldown_until, decision = "action-failed", "PostgreSQL maintenance action failed");
            Ok(PassOutcome::ActionFailed)
        }
        StatementOutcome::Succeeded => {
            let duration = started.elapsed();
            let comparison = read_comparison(tenant_connection, branch, baseline).await?;
            record_action(action, MaintenanceActionOutcome::Ok, duration, comparison.effect());
            let verdict = action_verdict(profile, target, comparison);
            let cooldown_until = cooldown_after(profile, target, ActionEnding::Completed(verdict), Utc::now());
            target_record::record_action_completion(platform, tenant, row, verdict, cooldown_until, comparison).await?;
            tenant_inspection::mark_inspected(platform, tenant, InspectionResult::Acted).await?;
            let completion = Completion {
                row,
                relation: &prepared.relation,
                baseline,
                comparison,
                verdict,
                cooldown_until,
                duration,
            };
            action_completion::report_completion(platform, tenant_connection, tenant, completion).await?;
            Ok(PassOutcome::Acted)
        }
    }
}

/// Withdraw every action queued in `branch`, which the visit only observes,
/// logging each as one automatic mode would run.
pub(crate) async fn withdraw_observed(platform: &mut PgConnection, tenant: &RegisteredTenant, branch: Branch) -> Result<(), MaintenanceError> {
    for target in pending_action::withdraw_actions(platform, tenant, branch).await? {
        info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?branch, target = target.relation_name(), action = ?target_action(target), decision = "would-act", "PostgreSQL maintenance withdrew a queued action from a branch it only observes");
    }
    Ok(())
}

/// Drop the pending action of `row`, whose reading no longer meets the
/// threshold that queued it: nothing runs, and a later sample queues it again
/// if it qualifies again.
pub(crate) async fn withdraw_lapsed(
    platform: &mut PgConnection,
    tenant: &RegisteredTenant,
    row: BranchTarget,
    reason: DeferralReason,
) -> Result<(), MaintenanceError> {
    let target = row.target();
    target_record::record_lapsed_action(platform, tenant, row, reason).await?;
    info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?row.branch(), target = target.relation_name(), action = ?target_action(target), reason = ?reason, decision = "no-longer-qualifies", "PostgreSQL maintenance pending action dropped");
    Ok(())
}

/// Drop a pending action whose live reading, taken just before it would run,
/// no longer meets the threshold that queued it, ending the pass idle.
async fn drop_lapsed(
    platform: &mut PgConnection,
    tenant: &RegisteredTenant,
    row: BranchTarget,
    reason: DeferralReason,
) -> Result<PassOutcome, MaintenanceError> {
    withdraw_lapsed(platform, tenant, row, reason).await?;
    tenant_inspection::mark_inspected(platform, tenant, InspectionResult::Idle).await?;
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
pub(crate) const fn target_action(target: MaintenanceTarget) -> MaintenanceAction {
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
        MaintenanceTarget::Repack(
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
        ) => MaintenanceAction::Repack,
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
