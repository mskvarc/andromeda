//! When a failed notification delivery becomes due again.
//!
//! The queue persists how many attempts a delivery has failed, so the wait is
//! read from that count rather than from a streak held in memory: the failure
//! being acknowledged is the retry numbered one past the persisted count.

use andromeda_retry::{
    retry_number::{RetryNumber, RetryNumberError},
    schedule::BackoffSchedule,
};
use chrono::{DateTime, TimeDelta, Utc};

/// The instant a delivery that had failed `attempt_count` attempts before the
/// one now failing is due again, counted from `now`.
///
/// A wait past what the calendar represents makes the delivery due at the end
/// of time, which no claim reaches.
///
/// # Errors
///
/// Returns [`RetryNumberError`] when `attempt_count` is negative, which the
/// queue never records.
pub fn retry_due_at(schedule: &BackoffSchedule, attempt_count: i32, now: DateTime<Utc>) -> Result<DateTime<Utc>, RetryNumberError> {
    let retry = RetryNumber::try_from(attempt_count.saturating_add(1))?;
    Ok(TimeDelta::from_std(schedule.delay(retry))
        .ok()
        .and_then(|wait| now.checked_add_signed(wait))
        .unwrap_or(DateTime::<Utc>::MAX_UTC))
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::delivery_backoff::retry_due_at;
    use andromeda_retry::{growth::Growth, retry_number::RetryNumber, schedule::BackoffSchedule};
    use chrono::{DateTime, TimeDelta, TimeZone, Utc};
    use pretty_assertions::assert_eq;
    use std::{num::NonZeroU32, time::Duration};

    /// Five seconds doubling, for a queue that allows five attempts.
    fn schedule() -> BackoffSchedule {
        BackoffSchedule::growing_until(Duration::from_secs(5), Growth::Doubling, RetryNumber::new(NonZeroU32::MIN.saturating_add(5)))
    }

    fn now() -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000, 0).single().expect("a valid instant")
    }

    #[test]
    fn the_first_failure_waits_exactly_the_base_backoff() {
        assert_eq!(retry_due_at(&schedule(), 0, now()), Ok(now() + TimeDelta::seconds(5)));
    }

    #[test]
    fn each_further_failure_doubles_the_wait_until_the_retry_limit() {
        let waits: Vec<_> = [1, 2, 3, 5, 10]
            .into_iter()
            .map(|attempts| retry_due_at(&schedule(), attempts, now()).map(|due| (due - now()).num_seconds()))
            .collect();
        assert_eq!(waits, [Ok(10), Ok(20), Ok(40), Ok(160), Ok(160)]);
    }

    #[test]
    fn a_negative_attempt_count_is_refused() {
        assert!(retry_due_at(&schedule(), -2, now()).is_err());
    }

    #[test]
    fn a_wait_past_the_calendar_is_due_at_the_end_of_time() {
        let schedule = BackoffSchedule::constant(Duration::MAX);
        assert_eq!(retry_due_at(&schedule, 0, now()), Ok(DateTime::<Utc>::MAX_UTC));
    }
}
