//! Cheap catalog evidence recorded on every visit: each heap's dead-tuple
//! estimate, each density-measured GIN's density, the freshness of each
//! heap's planner statistics and the partial chunks of each history
//! hypertable.

use crate::{
    branch::{Branch, BranchTarget},
    candidate::{MaintenanceTarget, heap_target::HeapTarget},
    columnstore_partiality::ColumnstorePartiality,
    controller::{PassOutcome, idle_gates},
    error::MaintenanceError,
    inspection::HeapSample,
    policy::{DeferralReason, DegradationSample, PolicyDecision, PolicyProfile, decide, decide_analyze, decide_recompression},
    state::{RegisteredTenant, target_record},
    statistics_freshness::HeapStatistics,
};
use andromeda_configuration::schema::database::maintenance::MaintenanceMode;
use chrono::Utc;
use sqlx::PgConnection;
use tracing::info;

/// One target's sample read from the catalogs, without a detailed probe.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CatalogSample {
    /// The sampled target.
    pub(crate) target: MaintenanceTarget,
    /// What the sample measured.
    pub(crate) evidence: SampleEvidence,
}

/// What one catalog sample measured, which decides how it qualifies.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SampleEvidence {
    /// Threshold-ready degradation of a heap or a density-measured GIN.
    Degradation(DegradationSample),
    /// A heap's modified tuples against its analyze trigger.
    Statistics(HeapStatistics),
    /// A history hypertable's partial chunks.
    Partiality(ColumnstorePartiality),
}

impl CatalogSample {
    /// A heap's dead-tuple estimate.
    pub(crate) const fn of_heap(sample: &HeapSample) -> CatalogSample {
        CatalogSample {
            target: MaintenanceTarget::Heap(sample.target),
            evidence: SampleEvidence::Degradation(sample.degradation),
        }
    }

    /// A heap target's planner statistics against its analyze trigger.
    pub(crate) const fn of_statistics(heap: HeapTarget, statistics: HeapStatistics) -> CatalogSample {
        CatalogSample {
            target: MaintenanceTarget::Statistics(heap),
            evidence: SampleEvidence::Statistics(statistics),
        }
    }

    /// A history hypertable's partial chunks.
    pub(crate) const fn of_partiality(partiality: ColumnstorePartiality) -> CatalogSample {
        CatalogSample {
            target: MaintenanceTarget::Columnstore(partiality.table),
            evidence: SampleEvidence::Partiality(partiality),
        }
    }
}

/// Persist the cheap catalog evidence of `branch` without executing newly
/// queued work in this pass.
pub(crate) async fn record_catalog_samples(
    mode: MaintenanceMode,
    platform: &mut PgConnection,
    tenant: &RegisteredTenant,
    branch: Branch,
    samples: &[CatalogSample],
    profile: PolicyProfile,
) -> Result<Option<PassOutcome>, MaintenanceError> {
    let mut outcome = None;
    for sample in samples {
        let target = sample.target;
        let row = BranchTarget::new(branch, target)?;
        let target_state = target_record::load_target_state(platform, tenant, row).await?;
        let now = Utc::now();
        let decision = match sample.evidence {
            SampleEvidence::Degradation(degradation) => {
                let decision = decide(mode, profile, target_state, degradation, idle_gates(), now);
                target_record::record_decision(platform, tenant, row, Some(degradation), decision, target_state.breach, now).await?;
                info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?branch, target = target.relation_name(), relation_bytes = degradation.relation_bytes, reclaimable_bytes = degradation.reclaimable_bytes, degraded_percent = degradation.degraded_percent, decision = ?decision, "PostgreSQL maintenance catalog sample completed");
                decision
            }
            SampleEvidence::Statistics(statistics) => {
                let freshness = statistics.freshness(now);
                let decision = decide_analyze(mode, profile, target_state, freshness, idle_gates(), now);
                target_record::record_decision(platform, tenant, row, None, decision, target_state.breach, now).await?;
                info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?branch, target = target.relation_name(), modified_since_analyze = statistics.modified_since_analyze, analyze_trigger = statistics.analyze_trigger, last_analyzed = ?statistics.last_analyzed, freshness = ?freshness, decision = ?decision, "PostgreSQL maintenance statistics sample completed");
                decision
            }
            SampleEvidence::Partiality(partiality) => {
                let decision = decide_recompression(mode, profile, target_state, partiality.state(), idle_gates(), now);
                target_record::record_decision(platform, tenant, row, None, decision, target_state.breach, now).await?;
                info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?branch, target = target.relation_name(), partial_chunks = partiality.partial_chunks, oldest_uncompressed_bytes = partiality.oldest_uncompressed_bytes, decision = ?decision, "PostgreSQL maintenance columnstore sample completed");
                decision
            }
        };
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
