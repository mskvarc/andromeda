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
    fn jitter_for(self, name: &TaskName) -> Duration {
        let Some(maximum) = self.maximum_jitter else {
            return Duration::ZERO;
        };
        let mut hasher = DefaultHasher::new();
        name.hash(&mut hasher);
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
    use crate::periodic_trigger::{InitialExecution, MissedTickPolicy, PeriodicSchedule, PeriodicScheduleError};
    use pretty_assertions::assert_eq;
    use std::time::Duration;

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
}
