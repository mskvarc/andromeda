//! Counting a suppression window back from an instant.
//!
//! Two spec members suppress a notification by comparing a stored instant
//! against the width of a window ending now: clause 5.2.12 `throttling` over
//! `lastNotification`, and Table 5.2.15-1 `cooldown` over `lastFailure`. The
//! widths are read from different members and computed differently; the
//! subtraction that turns a width into the ceiling a claim is gated on is the
//! same one, and is made here once.

use chrono::{DateTime, TimeDelta, Utc};

/// The latest instant that still lies outside a window of `width` ending at
/// `now`.
///
/// A stored instant at or before the returned ceiling is outside the window and
/// permits a notification; one after it is inside and suppresses one.
///
/// A window too wide for the calendar to express saturates at the earliest
/// representable instant, so every prior instant lies inside it: an
/// unrepresentable window suppresses maximally rather than not at all.
pub(crate) fn window_ceiling(now: DateTime<Utc>, width: Option<TimeDelta>) -> DateTime<Utc> {
    width.and_then(|width| now.checked_sub_signed(width)).unwrap_or(DateTime::<Utc>::MIN_UTC)
}

#[cfg(test)]
mod tests {
    use crate::suppression_window::window_ceiling;
    use chrono::{DateTime, Duration, TimeDelta, TimeZone, Utc};
    use pretty_assertions::assert_eq;

    #[test]
    fn the_ceiling_is_the_window_subtracted_from_the_current_instant() -> Result<(), &'static str> {
        let now = Utc.timestamp_opt(1_000, 0).single().ok_or("the fixture timestamp must be valid")?;
        assert_eq!(window_ceiling(now, TimeDelta::try_milliseconds(1_500)), now - Duration::milliseconds(1_500));
        Ok(())
    }

    /// A width the calendar cannot express suppresses maximally: every prior
    /// instant lies after the earliest representable one.
    #[test]
    fn an_unrepresentable_window_saturates_at_the_earliest_instant() -> Result<(), &'static str> {
        let now = Utc.timestamp_opt(10, 0).single().ok_or("the fixture timestamp must be valid")?;
        assert_eq!(window_ceiling(now, None), DateTime::<Utc>::MIN_UTC);
        Ok(())
    }
}
