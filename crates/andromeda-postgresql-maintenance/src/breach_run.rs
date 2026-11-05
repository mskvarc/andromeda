//! The unbroken run of breaching samples a target qualifies on.
//!
//! One sample is a single reading, never evidence of a lasting state, so
//! work qualifies only once a run of samples meeting every threshold has
//! lasted the qualifying duration over at least two samples. A sample under
//! any threshold closes the run, and a completed action closes it too.

use crate::policy::{DeferralReason, DegradationSample, PolicyDecision};
use chrono::{DateTime, Duration as ChronoDuration, Utc};

/// Fewest samples a breach run needs to qualify, however long it spans: one
/// sample is a single reading, never evidence of a lasting state.
pub const MINIMUM_BREACH_SAMPLES: u32 = 2;

/// An unbroken run of samples that met every degradation threshold.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BreachRun {
    /// When the run's first sample was taken.
    pub since: DateTime<Utc>,
    /// Samples in the run, at least one.
    pub samples: u32,
}

impl BreachRun {
    /// The run once a sample taken at `now` breaches too: the open run
    /// lengthened, or a new run starting at `now`.
    #[must_use]
    pub const fn extended(run: Option<BreachRun>, now: DateTime<Utc>) -> BreachRun {
        match run {
            Some(run) => BreachRun {
                since: run.since,
                samples: run.samples.saturating_add(1),
            },
            None => BreachRun { since: now, samples: 1 },
        }
    }

    /// Whether the run has lasted `qualifying_duration`, inclusive, over at
    /// least [`MINIMUM_BREACH_SAMPLES`] samples.
    #[must_use]
    pub fn qualifies(self, qualifying_duration: ChronoDuration, now: DateTime<Utc>) -> bool {
        self.samples >= MINIMUM_BREACH_SAMPLES && now.signed_duration_since(self.since) >= qualifying_duration
    }
}

/// The breach run to persist after `decision`, judged on a sample taken at
/// `now` (`sample` is `None` when a pending action was decided on without
/// one).
///
/// A breaching sample lengthens the run and a sample under any threshold
/// closes it, a repack's vacuum-pending and size-ceiling verdicts included,
/// which judge the sample. A decision taken before the sample was judged
/// (cooldown, a load gate, a pending action, a repack's own gates) leaves the
/// run as it was.
#[must_use]
pub fn breach_after(breach: Option<BreachRun>, decision: PolicyDecision, sample: Option<DegradationSample>, now: DateTime<Utc>) -> Option<BreachRun> {
    match decision {
        PolicyDecision::Queued | PolicyDecision::Deferred(DeferralReason::ConsecutiveSamples) => Some(BreachRun::extended(breach, now)),
        PolicyDecision::WouldAct => sample.map_or(breach, |_| Some(BreachRun::extended(breach, now))),
        PolicyDecision::Deferred(
            DeferralReason::RelationTooSmall
            | DeferralReason::ReclaimTooSmall
            | DeferralReason::BelowThreshold
            | DeferralReason::VacuumPending
            | DeferralReason::RelationTooLarge,
        ) => None,
        PolicyDecision::Act
        | PolicyDecision::Deferred(
            DeferralReason::Cooldown
            | DeferralReason::ActiveBackendRatio
            | DeferralReason::LongTransaction
            | DeferralReason::HorizonHeld
            | DeferralReason::ActiveMaintenance
            | DeferralReason::WaitingRelationLock
            | DeferralReason::ReplicationRetention
            | DeferralReason::SnapshotLifetimeShort
            | DeferralReason::RepackLeftover
            | DeferralReason::FreezePressure
            | DeferralReason::RepackRunning,
        ) => breach,
    }
}
