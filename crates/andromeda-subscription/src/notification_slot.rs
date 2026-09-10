//! What a subscription's stored delivery instants must satisfy for it to send
//! another notification.
//!
//! Two spec members gate the same claim from opposite directions: clause 5.2.12
//! `throttling` bounds how recently the subscription last notified, and Table
//! 5.2.15-1 `cooldown` bounds how recently its endpoint last failed. Both are
//! upper bounds on a stored instant, so they are computed together here and
//! handed to the store as one value rather than as two positional timestamps a
//! caller could swap.

use crate::{cooldown::ceiling_for_cooldown, throttle::ceiling_for_throttle};
use andromeda_ngsi_ld_lexicon::positive_milliseconds::PositiveMilliseconds;
use chrono::{DateTime, Utc};

/// The ceilings one notification-slot claim is gated on.
///
/// A claim succeeds only when the subscription's stored `last_notification` is
/// at or before [`NotificationSlotGate::last_notification`] and its stored
/// `last_failure` is at or before [`NotificationSlotGate::last_failure`]. `None`
/// on either side is no ceiling at all, which is what an unthrottled
/// subscription and an endpoint without a cooldown each carry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NotificationSlotGate {
    /// Latest `last_notification` that still permits a claim (clause 5.2.12
    /// `throttling`).
    pub last_notification: Option<DateTime<Utc>>,
    /// Latest `last_failure` that still permits a claim (Table 5.2.15-1
    /// `cooldown`).
    pub last_failure: Option<DateTime<Utc>>,
}

impl NotificationSlotGate {
    /// The gate a subscription's throttling interval and endpoint cooldown
    /// impose at `now`.
    #[must_use]
    pub fn at(now: DateTime<Utc>, throttling_secs: Option<f64>, cooldown: Option<PositiveMilliseconds>) -> NotificationSlotGate {
        NotificationSlotGate {
            last_notification: ceiling_for_throttle(now, throttling_secs),
            last_failure: ceiling_for_cooldown(now, cooldown),
        }
    }
}

/// The answer to one notification-slot claim.
///
/// The claim is the moment a notification either happens or does not: Table
/// 5.2.15-1 says of `cooldown` that "if requests are received before the
/// cooldown period has expired, no notification is sent", and clause 5.2.12
/// `throttling` bounds the same claim from the other side. Nothing downstream
/// has to be told which of the two refused it -- only that nothing is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationSlot {
    /// The slot was taken: the delivery instants moved and the notification is
    /// the caller's to send.
    Claimed,
    /// A gate refused it, so no notification is sent at all and the delivery
    /// instants did not move.
    Suppressed,
}

#[cfg(test)]
mod tests {
    use crate::notification_slot::NotificationSlotGate;
    use andromeda_ngsi_ld_lexicon::positive_milliseconds::PositiveMilliseconds;
    use chrono::{Duration, TimeZone, Utc};

    #[test]
    fn a_subscription_with_neither_member_is_gated_by_nothing() -> Result<(), &'static str> {
        let now = Utc.timestamp_opt(10, 0).single().ok_or("the fixture timestamp must be valid")?;
        pretty_assertions::assert_eq!(NotificationSlotGate::at(now, None, None), NotificationSlotGate::default());
        Ok(())
    }

    /// The two members are independent: one being set must not compute or
    /// clear the other's ceiling.
    #[test]
    fn each_member_sets_only_its_own_ceiling() -> Result<(), &'static str> {
        let now = Utc.timestamp_opt(1_000, 0).single().ok_or("the fixture timestamp must be valid")?;
        let cooldown = PositiveMilliseconds::try_from(2_000).map_err(|_error| "a positive millisecond count")?;

        let throttled = NotificationSlotGate::at(now, Some(5.0), None);
        pretty_assertions::assert_eq!(throttled.last_notification, Some(now - Duration::seconds(5)));
        assert!(throttled.last_failure.is_none());

        let cooled = NotificationSlotGate::at(now, None, Some(cooldown));
        assert!(cooled.last_notification.is_none());
        pretty_assertions::assert_eq!(cooled.last_failure, Some(now - Duration::milliseconds(2_000)));

        let both = NotificationSlotGate::at(now, Some(5.0), Some(cooldown));
        pretty_assertions::assert_eq!(both.last_notification, throttled.last_notification);
        pretty_assertions::assert_eq!(both.last_failure, cooled.last_failure);
        Ok(())
    }
}
