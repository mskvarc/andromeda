//! Cancellation-aware execution and recovery for qualified maintenance work.

use crate::{
    artifact::{ArtifactIndexName, ArtifactKind},
    candidate::{HeapTarget, IndexTarget, MaintenanceTarget},
    controller::PassOutcome,
    error::MaintenanceError,
    inspection,
    policy::{DeferralReason, DegradationSample, PolicyDecision, PolicyProfile, decide, safety_deferral},
    state::{self, RegisteredTenant},
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
    artifact: ArtifactIndexName,
    kind: ArtifactKind,
    profile: PolicyProfile,
    cancellation: &CancellationToken,
) -> Result<PassOutcome, MaintenanceError> {
    let target = MaintenanceTarget::Index(artifact.owner());
    let gates = inspection::safety_gates(tenant_connection, target).await?;
    if let Some(reason) = safety_deferral(profile, gates) {
        state::mark_inspected(platform, tenant, "deferred").await?;
        info!(tenant = tenant.log_name(), database = %tenant.database, target = artifact.as_str(), artifact_kind = ?kind, decision = "deferred", reason = ?reason, "PostgreSQL maintenance recovery deferred");
        return Ok(PassOutcome::Deferred);
    }
    if mode != MaintenanceMode::Automatic {
        state::mark_inspected(platform, tenant, "would-act").await?;
        info!(tenant = tenant.log_name(), database = %tenant.database, target = artifact.as_str(), artifact_kind = ?kind, decision = "would-drop-artifact", "PostgreSQL maintenance recovery observed");
        return Ok(PassOutcome::WouldAct);
    }
    let statement = artifact.drop_statement();
    let started = Instant::now();
    let result = execute_statement(tenant_connection, &statement, cancellation).await;
    match result {
        StatementOutcome::Succeeded => {
            record_action(MaintenanceAction::Recovery, MaintenanceActionOutcome::Succeeded, started.elapsed(), None);
            state::mark_inspected(platform, tenant, "acted").await?;
            info!(tenant = tenant.log_name(), database = %tenant.database, target = artifact.as_str(), artifact_kind = ?kind, decision = "dropped-artifact", "PostgreSQL concurrent-index recovery completed");
            Ok(PassOutcome::Acted)
        }
        StatementOutcome::Cancelled => {
            record_action(MaintenanceAction::Recovery, MaintenanceActionOutcome::Cancelled, started.elapsed(), None);
            Ok(PassOutcome::Cancelled)
        }
        StatementOutcome::Failed(error) => {
            record_action(MaintenanceAction::Recovery, MaintenanceActionOutcome::Failed, started.elapsed(), None);
            state::mark_inspected(platform, tenant, "action-failed").await?;
            warn!(tenant = tenant.log_name(), database = %tenant.database, target = artifact.as_str(), artifact_kind = ?kind, error = %error, decision = "artifact-drop-failed", "PostgreSQL concurrent-index recovery failed");
            Ok(PassOutcome::ActionFailed)
        }
    }
}

/// Execute one action qualified on an earlier pass.
pub(crate) async fn execute_pending(
    mode: MaintenanceMode,
    platform: &mut PgConnection,
    tenant_connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    target: MaintenanceTarget,
    profile: PolicyProfile,
    cancellation: &CancellationToken,
) -> Result<PassOutcome, MaintenanceError> {
    let gates = inspection::safety_gates(tenant_connection, target).await?;
    let target_state = state::load_target_state(platform, tenant, target).await?;
    let decision = decide(mode, profile, target_state, empty_sample(), gates, Utc::now());
    state::record_decision(platform, tenant, target, None, decision).await?;
    match decision {
        PolicyDecision::WouldAct => {
            state::mark_inspected(platform, tenant, "would-act").await?;
            info!(tenant = tenant.log_name(), database = %tenant.database, target = target.state_key(), decision = "would-act", "PostgreSQL maintenance pending action observed");
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
            | DeferralReason::ActiveMaintenance
            | DeferralReason::WaitingRelationLock),
        ) => {
            state::mark_inspected(platform, tenant, "deferred").await?;
            info!(tenant = tenant.log_name(), database = %tenant.database, target = target.state_key(), decision = "deferred", reason = ?reason, "PostgreSQL maintenance pending action deferred");
            return Ok(PassOutcome::Deferred);
        }
        PolicyDecision::Queued => {
            state::mark_inspected(platform, tenant, "queued").await?;
            return Ok(PassOutcome::Queued);
        }
        PolicyDecision::Act => {}
    }
    let before = inspection::relation_bytes(tenant_connection, target).await?;
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
            state::record_action_failure(platform, tenant, target).await?;
            state::mark_inspected(platform, tenant, "action-failed").await?;
            warn!(tenant = tenant.log_name(), database = %tenant.database, target = target.state_key(), error = %error, duration_seconds = started.elapsed().as_secs_f64(), decision = "action-failed", "PostgreSQL maintenance action failed");
            Ok(PassOutcome::ActionFailed)
        }
        StatementOutcome::Succeeded => {
            let after = inspection::relation_bytes(tenant_connection, target).await?;
            record_action(
                action,
                MaintenanceActionOutcome::Succeeded,
                started.elapsed(),
                Some(before.saturating_sub(after)),
            );
            state::record_action_success(platform, tenant, target, Utc::now() + profile.successful_cooldown, before, after).await?;
            state::mark_inspected(platform, tenant, "acted").await?;
            info!(tenant = tenant.log_name(), database = %tenant.database, target = target.state_key(), before_bytes = before, after_bytes = after, reclaimed_bytes = before.saturating_sub(after), duration_seconds = started.elapsed().as_secs_f64(), decision = "acted", "PostgreSQL maintenance action completed");
            Ok(PassOutcome::Acted)
        }
    }
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

/// Execute a statement built by the PostgreSQL SeaQuery dialect extension.
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
            IndexTarget::UqCurrentAttributeValuesGrain
            | IndexTarget::IdxCavAttrNumber
            | IndexTarget::IdxCavAttrString
            | IndexTarget::IdxCavAttrStringTrgm
            | IndexTarget::IdxCavAttrBool
            | IndexTarget::IdxCavAttrDatetime
            | IndexTarget::IdxCavAttrUri
            | IndexTarget::IdxCavAttrGeo
            | IndexTarget::IdxCavAttrEntityGeo,
        ) => MaintenanceAction::Reindex,
        MaintenanceTarget::Heap(HeapTarget::CurrentAttributeValues | HeapTarget::EntityDocuments | HeapTarget::SubscriptionDeliveryState) => {
            MaintenanceAction::Vacuum
        }
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
