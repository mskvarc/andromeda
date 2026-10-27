//! Cooldown-window arithmetic for subscription notification slots.
//!
//! The persistence layer gates a slot claim with a pure timestamp comparison;
//! this converts the endpoint's configured cooldown into the latest
//! `last_failure` that still permits a notification.

use crate::suppression_window::window_ceiling;
use andromeda_ngsi_ld_lexicon::positive_milliseconds::PositiveMilliseconds;
use chrono::{DateTime, TimeDelta, Utc};

/// Compute the latest `last_failure` instant that still permits a notification
/// to be sent to the endpoint (Table 5.2.15-1 `cooldown`).
///
/// Table 5.2.15-1 defines the member as the "minimum period of time in
/// milliseconds which shall elapse before attempting to make a subsequent
/// notification to the same endpoint after failure", and adds that "if requests
/// are received before the cooldown period has expired, no notification is
/// sent". The rule is therefore suppression, not delay: `now < last_failure +
/// cooldown` skips the notification, which is `last_failure <= now - cooldown`
/// to send it. That subtraction is what this returns.
///
/// `None` only when the endpoint names no cooldown, in which case no prior
/// failure holds a notification back.
#[must_use]
pub fn ceiling_for_cooldown(now: DateTime<Utc>, cooldown: Option<PositiveMilliseconds>) -> Option<DateTime<Utc>> {
    let cooldown = cooldown?;
    Some(window_ceiling(now, TimeDelta::try_milliseconds(i64::from(cooldown))))
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::cooldown::ceiling_for_cooldown;
    use andromeda_ngsi_ld_lexicon::positive_milliseconds::PositiveMilliseconds;
    use chrono::{DateTime, Duration, TimeZone, Utc};
    use pretty_assertions::assert_eq;

    fn milliseconds(value: i64) -> PositiveMilliseconds {
        PositiveMilliseconds::try_from(value).expect("a positive millisecond count")
    }

    #[test]
    fn an_endpoint_without_a_cooldown_is_never_held_back() -> Result<(), &'static str> {
        let now = Utc.timestamp_opt(10, 0).single().ok_or("the fixture timestamp must be valid")?;
        assert_eq!(ceiling_for_cooldown(now, None), None);
        Ok(())
    }

    #[test]
    fn the_ceiling_is_the_cooldown_subtracted_from_the_current_instant() -> Result<(), &'static str> {
        let now = Utc.timestamp_opt(1_000, 0).single().ok_or("the fixture timestamp must be valid")?;
        assert_eq!(ceiling_for_cooldown(now, Some(milliseconds(1_500))), Some(now - Duration::milliseconds(1_500)));
        Ok(())
    }

    /// A window reaching past the earliest representable instant holds back a
    /// notification after any recorded failure.
    #[test]
    fn a_cooldown_beyond_the_calendar_suppresses_after_any_failure() -> Result<(), &'static str> {
        let now = Utc.timestamp_opt(1_000, 0).single().ok_or("the fixture timestamp must be valid")?;
        assert_eq!(ceiling_for_cooldown(now, Some(milliseconds(i64::MAX))), Some(DateTime::<Utc>::MIN_UTC));
        Ok(())
    }

    /// A failure inside the window is after the ceiling and suppresses the
    /// notification; one older than the window is at or before it and does not.
    #[test]
    fn a_failure_inside_the_window_is_after_the_ceiling_and_one_outside_it_is_not() -> Result<(), &'static str> {
        let now = Utc.timestamp_opt(1_000, 0).single().ok_or("the fixture timestamp must be valid")?;
        let ceiling = ceiling_for_cooldown(now, Some(milliseconds(2_000))).ok_or("a cooldown yields a ceiling")?;
        assert!(now - Duration::milliseconds(500) > ceiling, "a recent failure still suppresses");
        assert!(now - Duration::milliseconds(5_000) <= ceiling, "an elapsed window stops suppressing");
        Ok(())
    }
}
