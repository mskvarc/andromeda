//! Explicit periodic scheduling policy.

use crate::task_name::TaskName;
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    time::Duration,
};
use thiserror::Error;
use tokio::time::{Instant, Interval, MissedTickBehavior, interval_at};

/// When the first scheduled execution becomes eligible.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitialExecution {
    /// Execute immediately after registration.
    Immediate,
    /// Wait for one complete cadence before executing.
    AfterCadence,
}

/// How a periodic trigger treats ticks missed while an execution is active.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissedTickPolicy {
    /// Skip elapsed cadence boundaries and schedule from the next boundary.
    Skip,
    /// Delay the cadence from the moment the missed tick is observed.
    Delay,
}

/// Validated periodic cadence, initial behavior, missed-tick policy and jitter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeriodicSchedule {
    /// Non-zero interval between eligible executions.
    cadence: Duration,
    /// First-execution timing.
    initial_execution: InitialExecution,
    /// Timer behavior after an execution spans cadence boundaries.
    missed_tick_policy: MissedTickPolicy,
    /// Optional deterministic per-task trigger spread.
    maximum_jitter: Option<Duration>,
    /// Extra hash input that shifts the spread per scope.
    ///
    /// The jitter is derived from the task name alone, so the same task in two
    /// scopes (for instance one tenant runtime and another) would otherwise
    /// draw the identical offset and fire in lockstep. A per-scope seed folds into
    /// the hash so a fleet of tenants started in one burst spreads its background
    /// work rather than stampeding the shared database in synchronized waves.
    jitter_seed: u64,
}

impl PeriodicSchedule {
    /// Construct a non-zero periodic schedule without jitter.
    ///
    /// # Errors
    ///
    /// Returns [`PeriodicScheduleError::ZeroCadence`] when `cadence` is zero, because a zero cadence would make every scheduled execution immediately eligible.
    pub const fn new(
        cadence: Duration,
        initial_execution: InitialExecution,
        missed_tick_policy: MissedTickPolicy,
    ) -> Result<PeriodicSchedule, PeriodicScheduleError> {
        if cadence.is_zero() {
            return Err(PeriodicScheduleError::ZeroCadence);
        }
        Ok(PeriodicSchedule {
            cadence,
            initial_execution,
            missed_tick_policy,
            maximum_jitter: None,
            jitter_seed: 0,
        })
    }

    /// Add deterministic jitter bounded below one full cadence.
    ///
    /// # Errors
    ///
    /// Returns [`PeriodicScheduleError::JitterNotBelowCadence`] when `maximum_jitter` is greater than or equal to the configured cadence.
    pub fn with_jitter(mut self, maximum_jitter: Duration) -> Result<PeriodicSchedule, PeriodicScheduleError> {
        if maximum_jitter >= self.cadence {
            return Err(PeriodicScheduleError::JitterNotBelowCadence);
        }
        self.maximum_jitter = (!maximum_jitter.is_zero()).then_some(maximum_jitter);
        Ok(self)
    }

    /// Fold a per-scope seed into the jitter derivation.
    ///
    /// Two scopes running the same-named task draw distinct offsets, so a burst of
    /// tenant runtimes does not fire its shared background tasks in lockstep. A
    /// zero seed leaves the derivation on the task name alone.
    #[must_use]
    pub const fn seeded(mut self, jitter_seed: u64) -> PeriodicSchedule {
        self.jitter_seed = jitter_seed;
        self
    }

    /// Ensure the schedule carries at least `desired` spread, without exceeding
    /// half the cadence so the offset always stays well below one full period.
    ///
    /// Leaves an explicitly configured jitter untouched, and no-ops when `desired`
    /// is zero. Used to spread a scope's tasks by default while letting a task opt
    /// into a wider, explicitly chosen spread.
    #[must_use]
    pub fn with_default_jitter(mut self, desired: Duration) -> PeriodicSchedule {
        if self.maximum_jitter.is_some() || desired.is_zero() {
            return self;
        }
        let ceiling = self.cadence / 2;
        let jitter = desired.min(ceiling);
        self.maximum_jitter = (!jitter.is_zero()).then_some(jitter);
        self
    }

    /// Return the configured cadence.
    #[must_use]
    pub const fn cadence(self) -> Duration {
        self.cadence
    }

    /// Return the first-execution policy.
    #[must_use]
    pub const fn initial_execution(self) -> InitialExecution {
        self.initial_execution
    }

    /// Return the missed-tick policy.
    #[must_use]
    pub const fn missed_tick_policy(self) -> MissedTickPolicy {
        self.missed_tick_policy
    }

    /// Return the optional maximum jitter.
    #[must_use]
    pub const fn maximum_jitter(self) -> Option<Duration> {
        self.maximum_jitter
    }

    /// Construct the Tokio interval used by one named task.
    pub(crate) fn interval(self, name: &TaskName) -> Interval {
        let jitter = self.jitter_for(name);
        let first_delay = match self.initial_execution {
            InitialExecution::Immediate => jitter,
            InitialExecution::AfterCadence => self.cadence.saturating_add(jitter),
        };
        let mut interval = interval_at(Instant::now() + first_delay, self.cadence);
        interval.set_missed_tick_behavior(match self.missed_tick_policy {
            MissedTickPolicy::Skip => MissedTickBehavior::Skip,
            MissedTickPolicy::Delay => MissedTickBehavior::Delay,
        });
        interval
    }

    /// Derive stable per-task jitter without introducing a random source.
    ///
    /// The offset is a pure function of the task name and the per-scope seed, so
    /// it is stable across restarts yet distinct for the same task in different
    /// scopes.
    fn jitter_for(self, name: &TaskName) -> Duration {
        let Some(maximum) = self.maximum_jitter else {
            return Duration::ZERO;
        };
        let mut hasher = DefaultHasher::new();
        name.hash(&mut hasher);
        self.jitter_seed.hash(&mut hasher);
        let ceiling = maximum.as_nanos();
        let nanos = u128::from(hasher.finish()) % (ceiling + 1);
        Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
    }
}

/// Invalid periodic schedule configuration.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum PeriodicScheduleError {
    /// A periodic cadence cannot be zero.
    #[error("periodic task cadence must be non-zero")]
    ZeroCadence,
    /// Jitter must remain below one cadence.
    #[error("periodic task jitter must be below its cadence")]
    JitterNotBelowCadence,
}

#[cfg(test)]
mod tests {
    use crate::{
        periodic_trigger::{InitialExecution, MissedTickPolicy, PeriodicSchedule, PeriodicScheduleError},
        task_name::TaskName,
    };
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    fn jittered(cadence_secs: u64, jitter_secs: u64) -> PeriodicSchedule {
        PeriodicSchedule::new(Duration::from_secs(cadence_secs), InitialExecution::Immediate, MissedTickPolicy::Skip)
            .expect("non-zero cadence")
            .with_jitter(Duration::from_secs(jitter_secs))
            .expect("jitter below cadence")
    }

    fn task() -> TaskName {
        TaskName::try_from("entity-expiry").expect("valid task name")
    }

    #[test]
    fn zero_cadence_is_rejected() {
        assert_eq!(
            PeriodicSchedule::new(Duration::ZERO, InitialExecution::Immediate, MissedTickPolicy::Skip),
            Err(PeriodicScheduleError::ZeroCadence)
        );
    }

    #[test]
    fn jitter_must_remain_below_cadence() {
        let schedule = PeriodicSchedule::new(Duration::from_secs(1), InitialExecution::Immediate, MissedTickPolicy::Skip);
        assert!(schedule.is_ok());
        let Ok(schedule) = schedule else {
            return;
        };
        assert_eq!(schedule.with_jitter(Duration::from_secs(1)), Err(PeriodicScheduleError::JitterNotBelowCadence));
    }

    #[test]
    fn a_seed_shifts_the_offset_for_the_same_task() {
        // The same task in two scopes draws distinct offsets, so a tenant burst
        // does not fire it in lockstep; both stay within the configured spread.
        let schedule = jittered(10, 5);
        let first = schedule.seeded(1).jitter_for(&task());
        let second = schedule.seeded(999_999).jitter_for(&task());
        assert_ne!(first, second, "distinct seeds must shift the offset");
        assert!(
            first < Duration::from_secs(5) && second < Duration::from_secs(5),
            "the offset stays below the configured jitter"
        );
    }

    #[test]
    fn the_same_seed_is_stable_for_the_same_task() {
        // Determinism: the offset is a pure function of name and seed, so it does
        // not drift across restarts.
        let schedule = jittered(10, 5);
        assert_eq!(schedule.seeded(7).jitter_for(&task()), schedule.seeded(7).jitter_for(&task()));
    }

    #[test]
    fn a_schedule_without_jitter_has_no_offset_whatever_the_seed() {
        let schedule = PeriodicSchedule::new(Duration::from_secs(10), InitialExecution::Immediate, MissedTickPolicy::Skip).expect("non-zero cadence");
        assert_eq!(schedule.seeded(42).jitter_for(&task()), Duration::ZERO);
    }

    #[test]
    fn a_default_jitter_is_capped_at_half_the_cadence() {
        let schedule = PeriodicSchedule::new(Duration::from_secs(10), InitialExecution::Immediate, MissedTickPolicy::Skip).expect("non-zero cadence");
        assert_eq!(
            schedule.with_default_jitter(Duration::from_secs(30)).maximum_jitter(),
            Some(Duration::from_secs(5))
        );
    }

    #[test]
    fn a_default_jitter_leaves_an_explicit_jitter_untouched() {
        assert_eq!(
            jittered(10, 2).with_default_jitter(Duration::from_secs(30)).maximum_jitter(),
            Some(Duration::from_secs(2))
        );
    }

    #[test]
    fn a_zero_default_jitter_adds_no_spread() {
        let schedule = PeriodicSchedule::new(Duration::from_secs(10), InitialExecution::Immediate, MissedTickPolicy::Skip).expect("non-zero cadence");
        assert_eq!(schedule.with_default_jitter(Duration::ZERO).maximum_jitter(), None);
    }
}
