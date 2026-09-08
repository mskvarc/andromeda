//! Throttling-window arithmetic for subscription notification slots.
//!
//! The persistence layer claims slots with a pure timestamp comparison; the
//! policy layer converts the subscription's configured throttling interval
//! into the earliest `last_notification` that still permits a new slot.

use chrono::{DateTime, TimeDelta, Utc};
use std::time::Duration;

/// Compute the earliest `last_notification` timestamp that still allows a new
/// notification slot to be claimed, given the subscription's throttling window.
///
/// Returns `None` when throttling is disabled (no ceiling: any prior
/// `last_notification` is acceptable), when the configured window is not a
/// positive finite number of seconds, and when the window is too wide to be
/// subtracted from `now`.
///
/// The window is built from the fractional seconds themselves rather than from
/// a whole number of them, so sub-second throttling (e.g. `0.5`) stays precise.
#[must_use]
pub fn ceiling_for_throttle(now: DateTime<Utc>, throttling_secs: Option<f64>) -> Option<DateTime<Utc>> {
    let secs = throttling_secs?;
    if !secs.is_finite() || secs <= 0.0 {
        return None;
    }
    let window = TimeDelta::from_std(Duration::try_from_secs_f64(secs).ok()?).ok()?;
    now.checked_sub_signed(window)
}

#[cfg(test)]
mod tests {
    use crate::throttle::ceiling_for_throttle;
    use chrono::{Duration, TimeZone, Utc};

    #[test]
    fn absent_or_non_positive_throttling_has_no_ceiling() -> Result<(), &'static str> {
        let now = Utc.timestamp_opt(10, 0).single().ok_or("the fixture timestamp must be valid")?;
        assert_eq!(ceiling_for_throttle(now, None), None);
        assert_eq!(ceiling_for_throttle(now, Some(0.0)), None);
        assert_eq!(ceiling_for_throttle(now, Some(-1.0)), None);
        Ok(())
    }

    #[test]
    fn fractional_throttling_preserves_subsecond_precision() -> Result<(), &'static str> {
        let now = Utc.timestamp_opt(10, 0).single().ok_or("the fixture timestamp must be valid")?;
        assert_eq!(ceiling_for_throttle(now, Some(0.5)), Some(now - Duration::milliseconds(500)));
        Ok(())
    }
}
