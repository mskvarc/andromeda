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
/// Returns `None` only when throttling is disabled: no configured value, or one
/// that is not a positive finite number of seconds. `None` means no ceiling, so
/// any prior `last_notification` is acceptable.
///
/// The window is built from the fractional seconds themselves rather than from
/// a whole number of them, so sub-second throttling (e.g. `0.5`) stays precise.
/// A window too wide for the calendar to express saturates at the earliest
/// representable instant rather than collapsing to `None`: every prior
/// notification lies inside such a window, so the subscription is maximally
/// throttled, not unthrottled.
#[must_use]
pub fn ceiling_for_throttle(now: DateTime<Utc>, throttling_secs: Option<f64>) -> Option<DateTime<Utc>> {
    let secs = throttling_secs?;
    if !secs.is_finite() || secs <= 0.0 {
        return None;
    }
    let window = Duration::try_from_secs_f64(secs).ok().and_then(|window| TimeDelta::from_std(window).ok());
    Some(window.and_then(|window| now.checked_sub_signed(window)).unwrap_or(DateTime::<Utc>::MIN_UTC))
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

    #[test]
    fn a_throttling_window_wider_than_the_calendar_throttles_rather_than_disabling() -> Result<(), &'static str> {
        let now = Utc.timestamp_opt(10, 0).single().ok_or("the fixture timestamp must be valid")?;
        let ceiling = ceiling_for_throttle(now, Some(1e30)).ok_or("an unrepresentable window must still yield a ceiling")?;
        assert!(ceiling < now, "the ceiling must precede now so no prior notification can claim a slot");
        Ok(())
    }
}
