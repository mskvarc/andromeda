//! The detailed index probes of one visit, run in probe order within the
//! profile's probe budget.
//!
//! A probe starts only while the budget left covers one as long as the
//! longest this visit has taken, so a visit ends near its budget rather than
//! one whole probe past it. The first probe always starts. Cancellation and
//! the safety gates are checked before every probe.

use crate::{
    branch::BranchTarget,
    candidate::MaintenanceTarget,
    controller::{PassOutcome, idle_gates, visit_branches::VisitBranches},
    error::MaintenanceError,
    inspection::{self, IndexSize},
    policy::{DeferralReason, PolicyDecision, PolicyProfile, decide, safety_deferral},
    safety_gate_inspection::{self, GateScope},
    state::{RegisteredTenant, target_record},
};
use chrono::Utc;
use sqlx::PgConnection;
use std::{
    ops::ControlFlow,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use tracing::info;

/// What a visit's probes found, before the heap samples are weighed in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProbeRun {
    /// No index was large enough, out of cooldown and probeable.
    NoCandidate,
    /// The safety gates deferred the first probe.
    Deferred,
    /// Cancellation stopped the visit.
    Cancelled,
    /// At least one probe completed.
    Completed {
        /// [`PassOutcome::Queued`] if any probe queued work, else
        /// [`PassOutcome::WouldAct`] if any would act, else `None`.
        flagged: Option<PassOutcome>,
    },
}

impl ProbeRun {
    /// The pass outcome of a visit whose heap samples gave `sampling`.
    pub(crate) fn pass_outcome(self, sampling: Option<PassOutcome>) -> PassOutcome {
        match self {
            ProbeRun::NoCandidate => sampling.unwrap_or(PassOutcome::Idle),
            ProbeRun::Deferred => PassOutcome::Deferred,
            ProbeRun::Cancelled => PassOutcome::Cancelled,
            ProbeRun::Completed { flagged } => flagged.or(sampling).unwrap_or(PassOutcome::Probed),
        }
    }
}

/// Wall time a visit's probes have spent against its budget.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ProbeBudget {
    /// The profile's probe budget.
    limit: Duration,
    /// Time the completed probes took.
    spent: Duration,
    /// The longest completed probe.
    longest: Duration,
}

impl ProbeBudget {
    /// A budget of `limit` with nothing spent.
    const fn new(limit: Duration) -> ProbeBudget {
        ProbeBudget {
            limit,
            spent: Duration::ZERO,
            longest: Duration::ZERO,
        }
    }

    /// Charge one completed probe that took `took`, and break when the budget
    /// left cannot cover another as long as the longest so far.
    fn charge(&mut self, took: Duration) -> ControlFlow<()> {
        self.spent = self.spent.saturating_add(took);
        self.longest = self.longest.max(took);
        if self.spent.saturating_add(self.longest) > self.limit {
            return ControlFlow::Break(());
        }
        ControlFlow::Continue(())
    }
}

/// Probe `candidates` in order until the budget, the safety gates or
/// cancellation stop the visit, recording each probe's decision under the
/// mode of the candidate's branch.
pub(crate) async fn probe_indexes(
    branches: &VisitBranches,
    platform: &mut PgConnection,
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    candidates: &[IndexSize],
    profile: PolicyProfile,
    cancellation: &CancellationToken,
) -> Result<ProbeRun, MaintenanceError> {
    let mut budget = ProbeBudget::new(profile.probe_budget);
    let mut run = ProbeRun::NoCandidate;
    for &size in candidates {
        if cancellation.is_cancelled() {
            return Ok(ProbeRun::Cancelled);
        }
        let started = Instant::now();
        let target = MaintenanceTarget::Index(size.target);
        let row = BranchTarget::new(size.branch, target)?;
        let gates = safety_gate_inspection::safety_gates(connection, GateScope::Target(row)).await?;
        if let Some(reason) = safety_deferral(profile, gates) {
            info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?size.branch, target = target.relation_name(), decision = "deferred", reason = ?reason, "PostgreSQL maintenance probe deferred");
            return Ok(match run {
                ProbeRun::NoCandidate => ProbeRun::Deferred,
                ProbeRun::Deferred | ProbeRun::Cancelled | ProbeRun::Completed { .. } => run,
            });
        }
        let measured = tokio::select! {
            () = cancellation.cancelled() => return Ok(ProbeRun::Cancelled),
            result = inspection::probe_index(connection, size, profile) => result?,
        };
        let target_state = target_record::load_target_state(platform, tenant, row).await?;
        let now = Utc::now();
        let decision = decide(branches.mode(size.branch), profile, target_state, measured, idle_gates(), now);
        target_record::record_decision(platform, tenant, row, Some(measured), decision, target_state.breach, now).await?;
        let took = started.elapsed();
        info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?size.branch, target = target.relation_name(), relation_bytes = measured.relation_bytes, reclaimable_bytes = measured.reclaimable_bytes, degraded_percent = measured.degraded_percent, decision = ?decision, duration_seconds = took.as_secs_f64(), "PostgreSQL maintenance index probe completed");
        run = ProbeRun::Completed {
            flagged: flagged_after(run, decision),
        };
        if budget.charge(took).is_break() {
            break;
        }
    }
    Ok(run)
}

/// The flag a visit carries after one more probe decided `decision`.
const fn flagged_after(run: ProbeRun, decision: PolicyDecision) -> Option<PassOutcome> {
    let flagged = match run {
        ProbeRun::Completed { flagged } => flagged,
        ProbeRun::NoCandidate | ProbeRun::Deferred | ProbeRun::Cancelled => None,
    };
    match (flagged, decision) {
        (Some(PassOutcome::Queued), _) | (_, PolicyDecision::Queued) => Some(PassOutcome::Queued),
        (None, PolicyDecision::WouldAct | PolicyDecision::Act) => Some(PassOutcome::WouldAct),
        (
            _,
            PolicyDecision::WouldAct
            | PolicyDecision::Act
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
            ),
        ) => flagged,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        controller::{
            PassOutcome,
            index_probing::{ProbeBudget, ProbeRun, flagged_after},
        },
        policy::{DeferralReason, PolicyDecision},
    };
    use pretty_assertions::assert_eq;
    use std::{ops::ControlFlow, time::Duration};

    #[test]
    fn probe_budget_stops_between_probes() {
        let mut budget = ProbeBudget::new(Duration::from_secs(30));
        assert_eq!(
            budget.charge(Duration::from_secs(20)),
            ControlFlow::Break(()),
            "a second 20 s probe would end past 30 s"
        );
    }

    #[test]
    fn short_probes_continue_until_the_next_would_overrun() {
        let mut budget = ProbeBudget::new(Duration::from_secs(30));
        for _ in 0..4 {
            assert_eq!(budget.charge(Duration::from_secs(6)), ControlFlow::Continue(()));
        }
        assert_eq!(
            budget.charge(Duration::from_secs(6)),
            ControlFlow::Break(()),
            "30 s spent leaves no room for a sixth"
        );
    }

    #[test]
    fn a_queued_probe_outranks_a_later_one_that_would_act() {
        let queued = ProbeRun::Completed {
            flagged: flagged_after(ProbeRun::NoCandidate, PolicyDecision::Queued),
        };
        assert_eq!(flagged_after(queued, PolicyDecision::WouldAct), Some(PassOutcome::Queued));
        assert_eq!(
            flagged_after(ProbeRun::NoCandidate, PolicyDecision::Deferred(DeferralReason::BelowThreshold)),
            None
        );
    }

    #[test]
    fn a_visit_outcome_weighs_its_probes_ahead_of_its_heap_samples() {
        assert_eq!(ProbeRun::NoCandidate.pass_outcome(None), PassOutcome::Idle);
        assert_eq!(ProbeRun::NoCandidate.pass_outcome(Some(PassOutcome::Queued)), PassOutcome::Queued);
        assert_eq!(ProbeRun::Deferred.pass_outcome(Some(PassOutcome::Queued)), PassOutcome::Deferred);
        assert_eq!(ProbeRun::Completed { flagged: None }.pass_outcome(None), PassOutcome::Probed);
        assert_eq!(
            ProbeRun::Completed { flagged: None }.pass_outcome(Some(PassOutcome::WouldAct)),
            PassOutcome::WouldAct
        );
        assert_eq!(
            ProbeRun::Completed {
                flagged: Some(PassOutcome::Queued)
            }
            .pass_outcome(Some(PassOutcome::WouldAct)),
            PassOutcome::Queued
        );
    }
}
