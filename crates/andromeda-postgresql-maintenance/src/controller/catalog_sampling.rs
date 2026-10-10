//! Cheap catalog evidence recorded on every visit: each heap's dead-tuple
//! estimate and each density-measured GIN's density.

use crate::{
    candidate::MaintenanceTarget,
    controller::{PassOutcome, idle_gates},
    error::MaintenanceError,
    inspection::HeapSample,
    policy::{DeferralReason, DegradationSample, PolicyDecision, PolicyProfile, decide},
    state::{RegisteredTenant, target_record},
};
use andromeda_postgresql_context_store::maintenance_config::MaintenanceMode;
use chrono::Utc;
use sqlx::PgConnection;
use tracing::info;

/// One target's sample read from the catalogs, without a detailed probe.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CatalogSample {
    /// The sampled target.
    pub(crate) target: MaintenanceTarget,
    /// Threshold-ready degradation sample.
    pub(crate) degradation: DegradationSample,
}

impl CatalogSample {
    /// A heap's dead-tuple estimate.
    pub(crate) const fn of_heap(sample: &HeapSample) -> CatalogSample {
        CatalogSample {
            target: MaintenanceTarget::Heap(sample.target),
            degradation: sample.degradation,
        }
    }
}

/// Persist cheap catalog evidence without executing newly queued work in this
/// pass.
pub(crate) async fn record_catalog_samples(
    mode: MaintenanceMode,
    platform: &mut PgConnection,
    tenant: &RegisteredTenant,
    samples: &[CatalogSample],
    profile: PolicyProfile,
) -> Result<Option<PassOutcome>, MaintenanceError> {
    let mut outcome = None;
    for sample in samples {
        let target = sample.target;
        let target_state = target_record::load_target_state(platform, tenant, target).await?;
        let now = Utc::now();
        let decision = decide(mode, profile, target_state, sample.degradation, idle_gates(), now);
        target_record::record_decision(platform, tenant, target, Some(sample.degradation), decision, target_state.breach, now).await?;
        info!(tenant = tenant.log_name(), database = %tenant.database, target = target.relation_name(), relation_bytes = sample.degradation.relation_bytes, reclaimable_bytes = sample.degradation.reclaimable_bytes, degraded_percent = sample.degradation.degraded_percent, decision = ?decision, "PostgreSQL maintenance catalog sample completed");
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
