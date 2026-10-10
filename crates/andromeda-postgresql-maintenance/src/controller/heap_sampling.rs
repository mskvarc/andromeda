//! Cheap heap evidence recorded on every visit.

use crate::{
    candidate::MaintenanceTarget,
    controller::{PassOutcome, idle_gates},
    error::MaintenanceError,
    inspection::HeapSample,
    policy::{DeferralReason, PolicyDecision, PolicyProfile, decide},
    state::{RegisteredTenant, target_record},
};
use andromeda_postgresql_context_store::maintenance_config::MaintenanceMode;
use chrono::Utc;
use sqlx::PgConnection;
use tracing::info;

/// Persist cheap heap evidence without executing newly queued work in this pass.
pub(crate) async fn record_heap_samples(
    mode: MaintenanceMode,
    platform: &mut PgConnection,
    tenant: &RegisteredTenant,
    samples: &[HeapSample],
    profile: PolicyProfile,
) -> Result<Option<PassOutcome>, MaintenanceError> {
    let mut outcome = None;
    for sample in samples {
        let target = MaintenanceTarget::Heap(sample.target);
        let target_state = target_record::load_target_state(platform, tenant, target).await?;
        let now = Utc::now();
        let decision = decide(mode, profile, target_state, sample.degradation, idle_gates(), now);
        target_record::record_decision(platform, tenant, target, Some(sample.degradation), decision, target_state.breach, now).await?;
        info!(tenant = tenant.log_name(), database = %tenant.database, target = target.relation_name(), relation_bytes = sample.degradation.relation_bytes, reclaimable_bytes = sample.degradation.reclaimable_bytes, degraded_percent = sample.degradation.degraded_percent, decision = ?decision, "PostgreSQL maintenance heap sample completed");
        match decision {
            PolicyDecision::Queued => outcome = Some(PassOutcome::Queued),
            PolicyDecision::WouldAct if outcome.is_none() => outcome = Some(PassOutcome::WouldAct),
            PolicyDecision::Act
            | PolicyDecision::WouldAct
            | PolicyDecision::Deferred(
                DeferralReason::RelationTooSmall
                | DeferralReason::ReclaimTooSmall
                | DeferralReason::BelowThreshold
                | DeferralReason::ConsecutiveSamples
                | DeferralReason::Cooldown
                | DeferralReason::ActiveBackendRatio
                | DeferralReason::LongTransaction
                | DeferralReason::HorizonHeld
                | DeferralReason::ActiveMaintenance
                | DeferralReason::WaitingRelationLock,
            ) => {}
        }
    }
    Ok(outcome)
}
