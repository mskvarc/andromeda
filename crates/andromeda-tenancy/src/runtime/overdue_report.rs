//! Reporting the overdue wakes each wake pass leaves.
//!
//! The gauges are set after every pass, zero included, so an operator can
//! alert on them. While wakes stay overdue, a `warn` line names how many and
//! how long the oldest has waited, at most once a minute: a backlog lasts as
//! long as more tenants have work due than the broker can load, and one line
//! per pass would flood the log with passes that run back to back.

use crate::wake_backlog::OverdueWakes;
use andromeda_telemetry::tenancy::set_overdue_wakes;
use std::{
    sync::{Mutex, MutexGuard, PoisonError},
    time::{Duration, SystemTime},
};
use tracing::warn;

/// The least time between two warnings of overdue wakes.
const WARNING_INTERVAL: Duration = Duration::from_mins(1);

/// Whether a pass logs its overdue wakes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Warning {
    /// It logs them.
    Logged,
    /// None are overdue, or the last warning was less than a minute ago.
    Withheld,
}

/// The overdue wakes the last pass left, as the gauges and the log report them.
#[derive(Debug, Default)]
pub(super) struct OverdueReport {
    /// When the last warning was logged, if ever.
    warned_at: Mutex<Option<SystemTime>>,
}

impl OverdueReport {
    /// Report `overdue`, the wakes a pass that ran at `now` left overdue.
    pub(super) fn publish(&self, now: SystemTime, overdue: Option<OverdueWakes>) {
        let oldest_age = overdue.map_or(Duration::ZERO, |wakes| wakes.oldest_age_at(now));
        set_overdue_wakes(overdue.map_or(0, |wakes| wakes.count().get()), oldest_age.as_secs_f64());
        if let (Some(wakes), Warning::Logged) = (overdue, self.warning(now, overdue)) {
            warn!(
                overdue = wakes.count().get(),
                oldest_overdue_seconds = oldest_age.as_secs_f64(),
                "tenant wakes are overdue: more tenants have work due than the broker can load"
            );
        }
    }

    /// Whether a pass that ran at `now` and left `overdue` logs a warning,
    /// noting the warning when it does. A clock that went back since the last
    /// warning does not withhold the next.
    fn warning(&self, now: SystemTime, overdue: Option<OverdueWakes>) -> Warning {
        if overdue.is_none() {
            return Warning::Withheld;
        }
        let mut warned_at = self.lock();
        let recent = warned_at.is_some_and(|warned| now.duration_since(warned).is_ok_and(|elapsed| elapsed < WARNING_INTERVAL));
        if recent {
            return Warning::Withheld;
        }
        *warned_at = Some(now);
        Warning::Logged
    }

    /// The last warning's instant, whatever a panicking holder left: every
    /// write leaves a whole value.
    fn lock(&self) -> MutexGuard<'_, Option<SystemTime>> {
        self.warned_at.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        runtime::overdue_report::{OverdueReport, Warning},
        wake_backlog::OverdueWakes,
    };
    use andromeda_telemetry::{
        error::TelemetryError,
        names::{TENANT_OVERDUE_WAKE_MAX_AGE_SECONDS, TENANT_OVERDUE_WAKES},
        recorder::TelemetryRecorder,
    };
    use pretty_assertions::assert_eq;
    use std::{
        num::NonZeroUsize,
        time::{Duration, SystemTime},
    };

    fn at(seconds: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)
    }

    fn overdue(count: usize, oldest_due: SystemTime) -> Option<OverdueWakes> {
        NonZeroUsize::new(count).map(|count| OverdueWakes::new(count, oldest_due))
    }

    #[test]
    fn overdue_wakes_are_warned_of_at_most_once_a_minute() {
        let report = OverdueReport::default();
        let warnings: Vec<Warning> = [1_000, 1_030, 1_059, 1_060, 1_100, 1_121]
            .into_iter()
            .map(|seconds| report.warning(at(seconds), overdue(3, at(900))))
            .collect();

        assert_eq!(
            warnings,
            [
                Warning::Logged,
                Warning::Withheld,
                Warning::Withheld,
                Warning::Logged,
                Warning::Withheld,
                Warning::Logged
            ]
        );
    }

    #[test]
    fn a_pass_with_nothing_overdue_is_not_warned_of_and_does_not_hold_back_the_next_warning() {
        let report = OverdueReport::default();

        assert_eq!(report.warning(at(1_000), None), Warning::Withheld);
        assert_eq!(report.warning(at(1_010), overdue(1, at(1_005))), Warning::Logged);
        assert_eq!(report.warning(at(1_020), None), Warning::Withheld);
        assert_eq!(report.warning(at(1_030), overdue(1, at(1_005))), Warning::Withheld);
    }

    #[test]
    fn a_clock_that_went_back_does_not_silence_the_warning() {
        let report = OverdueReport::default();

        assert_eq!(report.warning(at(1_000), overdue(2, at(990))), Warning::Logged);
        assert_eq!(report.warning(at(500), overdue(2, at(490))), Warning::Logged);
    }

    #[test]
    fn the_gauges_hold_the_count_and_the_oldest_age_and_return_to_zero() -> Result<(), TelemetryError> {
        let report = OverdueReport::default();
        let behind = TelemetryRecorder::detached()?;
        behind.record_locally(|| report.publish(at(1_000), overdue(12, at(940))));
        let caught_up = TelemetryRecorder::detached()?;
        caught_up.record_locally(|| report.publish(at(1_005), None));

        let behind = behind.render();
        let caught_up = caught_up.render();
        assert!(behind.contains(&format!("{TENANT_OVERDUE_WAKES} 12")), "{behind}");
        assert!(behind.contains(&format!("{TENANT_OVERDUE_WAKE_MAX_AGE_SECONDS} 60")), "{behind}");
        assert!(caught_up.contains(&format!("{TENANT_OVERDUE_WAKES} 0")), "{caught_up}");
        assert!(caught_up.contains(&format!("{TENANT_OVERDUE_WAKE_MAX_AGE_SECONDS} 0")), "{caught_up}");
        Ok(())
    }
}
