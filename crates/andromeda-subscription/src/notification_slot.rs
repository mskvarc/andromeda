//! What a subscription's stored delivery instants must satisfy for it to send
//! another notification, and what recording that notification's outcome
//! writes.
//!
//! Two spec members gate the same claim from opposite directions: clause 5.2.12
//! `throttling` bounds how recently the subscription last notified, and Table
//! 5.2.15-1 `cooldown` bounds how recently its endpoint last failed. Both are
//! upper bounds on a stored instant, so they are computed together here and
//! handed to the store as one value rather than as two positional timestamps a
//! caller could swap.
//!
//! Clause 5.8.6 moves `timesSent` and `lastNotification` when a notification is
//! sent and `lastSuccess` or `lastFailure` once its response is known. Only a
//! sender that decides a later notification at the claim needs its row moved
//! before the send: a throttled subscription, whose next claim is gated on
//! `lastNotification`, and a `timeInterval` subscription, which records the
//! sweep instant the notification answers its grid at. Every other
//! notification is counted by the one write that records its outcome, so its
//! delivery-state row is written once per notification.

use crate::{cooldown::ceiling_for_cooldown, throttle::ceiling_for_throttle};
use andromeda_ngsi_ld_lexicon::positive_milliseconds::PositiveMilliseconds;
use chrono::{DateTime, Utc};

/// How a subscription comes to notify (Table 5.2.12-1 makes `throttling` and
/// `timeInterval` mutually exclusive).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NotificationCadence {
    /// Notified on change, at most once per `throttling_secs` when it is set.
    OnChange {
        /// The subscription's `throttling` in seconds, if any.
        throttling_secs: Option<f64>,
    },
    /// Notified every `timeInterval`, on a grid anchored on the subscription's
    /// creation.
    Periodic,
}

/// When a claimed notification's `timesSent` and `lastNotification` move.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum NotificationStamp {
    /// With the outcome, in the one write that records it; the claim only
    /// checks the subscription may notify.
    #[default]
    WithOutcome,
    /// At the claim, before the notification is sent, gated on a stored
    /// `last_notification` at or before `ceiling` (clause 5.2.12
    /// `throttling`).
    Throttled {
        /// Latest `last_notification` that still permits a claim.
        ceiling: DateTime<Utc>,
    },
    /// At the claim, before the notification is sent, with `tick` recorded as
    /// the sweep instant the notification answers the `timeInterval` grid at
    /// (clause 5.8.6).
    Periodic {
        /// The sweep instant the notification is sent for.
        tick: DateTime<Utc>,
    },
}

/// The ceilings one notification-slot claim is gated on, and when it stamps
/// the notification.
///
/// A claim succeeds only when the subscription's stored `last_notification` is
/// at or before the [`NotificationStamp::Throttled`] ceiling and its stored
/// `last_failure` is at or before [`NotificationSlotGate::last_failure`]. `None`
/// on either side is no ceiling at all, which is what an unthrottled
/// subscription and an endpoint without a cooldown each carry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NotificationSlotGate {
    /// When the notification is stamped, with the throttling ceiling a stamp
    /// at the claim is gated on.
    pub notification: NotificationStamp,
    /// Latest `last_failure` that still permits a claim (Table 5.2.15-1
    /// `cooldown`).
    pub last_failure: Option<DateTime<Utc>>,
}

impl NotificationSlotGate {
    /// The gate a subscription of `cadence` and endpoint `cooldown` imposes at
    /// `now`.
    #[must_use]
    pub fn at(now: DateTime<Utc>, cadence: NotificationCadence, cooldown: Option<PositiveMilliseconds>) -> NotificationSlotGate {
        let notification = match cadence {
            NotificationCadence::Periodic => NotificationStamp::Periodic { tick: now },
            NotificationCadence::OnChange { throttling_secs } => match ceiling_for_throttle(now, throttling_secs) {
                Some(ceiling) => NotificationStamp::Throttled { ceiling },
                None => NotificationStamp::WithOutcome,
            },
        };
        NotificationSlotGate {
            notification,
            last_failure: ceiling_for_cooldown(now, cooldown),
        }
    }
}

/// What the write recording a notification's outcome adds to the outcome
/// itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutcomeRecord {
    /// The outcome alone: the claim already counted the notification, or none
    /// was sent.
    OutcomeOnly,
    /// The notification is counted with its outcome: `timesSent` advances and
    /// `lastNotification` moves to `notified_at` unless a later notification
    /// already moved it further.
    CountingNotification {
        /// The instant the slot was claimed, on the store's clock.
        notified_at: DateTime<Utc>,
    },
}

/// The answer to one notification-slot claim.
///
/// The claim is the moment a notification either happens or does not: Table
/// 5.2.15-1 says of `cooldown` that "if requests are received before the
/// cooldown period has expired, no notification is sent", and clause 5.2.12
/// `throttling` bounds the same claim from the other side. Nothing downstream
/// has to be told which of the two refused it, only that nothing is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationSlot {
    /// The slot was taken and the notification is the caller's to send; its
    /// outcome is recorded with the [`OutcomeRecord`] carried.
    Claimed(OutcomeRecord),
    /// A gate refused it, or the subscription is no longer active, so no
    /// notification is sent at all and the delivery instants did not move.
    Suppressed,
}

/// How the endpoint answered one notification (clause 5.8.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationOutcome {
    /// The endpoint accepted the notification: `lastSuccess` moves.
    Success,
    /// The endpoint refused it or could not be reached: `lastFailure` moves
    /// and `timesFailed` advances.
    Failure,
}

#[cfg(test)]
mod tests {
    use crate::notification_slot::{NotificationCadence, NotificationSlotGate, NotificationStamp};
    use andromeda_ngsi_ld_lexicon::positive_milliseconds::PositiveMilliseconds;
    use chrono::{Duration, TimeZone, Utc};

    const UNTHROTTLED: NotificationCadence = NotificationCadence::OnChange { throttling_secs: None };

    #[test]
    fn a_subscription_with_neither_member_is_gated_by_nothing() -> Result<(), &'static str> {
        let now = Utc.timestamp_opt(10, 0).single().ok_or("the fixture timestamp must be valid")?;
        pretty_assertions::assert_eq!(NotificationSlotGate::at(now, UNTHROTTLED, None), NotificationSlotGate::default());
        Ok(())
    }

    /// The two members are independent: one being set must not compute or
    /// clear the other's ceiling.
    #[test]
    fn each_member_sets_only_its_own_ceiling() -> Result<(), &'static str> {
        let now = Utc.timestamp_opt(1_000, 0).single().ok_or("the fixture timestamp must be valid")?;
        let cooldown = PositiveMilliseconds::try_from(2_000).map_err(|_error| "a positive millisecond count")?;
        let throttling = NotificationCadence::OnChange { throttling_secs: Some(5.0) };

        let throttled = NotificationSlotGate::at(now, throttling, None);
        pretty_assertions::assert_eq!(
            throttled.notification,
            NotificationStamp::Throttled {
                ceiling: now - Duration::seconds(5)
            }
        );
        assert!(throttled.last_failure.is_none());

        let cooled = NotificationSlotGate::at(now, UNTHROTTLED, Some(cooldown));
        pretty_assertions::assert_eq!(cooled.notification, NotificationStamp::WithOutcome);
        pretty_assertions::assert_eq!(cooled.last_failure, Some(now - Duration::milliseconds(2_000)));

        let both = NotificationSlotGate::at(now, throttling, Some(cooldown));
        pretty_assertions::assert_eq!(both.notification, throttled.notification);
        pretty_assertions::assert_eq!(both.last_failure, cooled.last_failure);
        Ok(())
    }

    /// A periodic claim records the sweep instant it answers the grid at, with
    /// no throttling ceiling (Table 5.2.12-1 gives `throttling` cardinality 0
    /// beside `timeInterval`).
    #[test]
    fn a_periodic_subscription_records_its_sweep_instant_at_the_claim() -> Result<(), &'static str> {
        let now = Utc.timestamp_opt(1_000, 0).single().ok_or("the fixture timestamp must be valid")?;
        let periodic = NotificationSlotGate::at(now, NotificationCadence::Periodic, None);
        pretty_assertions::assert_eq!(periodic.notification, NotificationStamp::Periodic { tick: now });
        assert!(periodic.last_failure.is_none());
        Ok(())
    }

    /// Only a throttled subscription decides its next claim by
    /// `lastNotification`, so an unthrottled one counts its notification with
    /// the outcome even when its endpoint has a cooldown, which reads
    /// `lastFailure` alone.
    #[test]
    fn an_unthrottled_subscription_counts_its_notification_with_the_outcome() -> Result<(), &'static str> {
        let now = Utc.timestamp_opt(1_000, 0).single().ok_or("the fixture timestamp must be valid")?;
        let cooldown = PositiveMilliseconds::try_from(2_000).map_err(|_error| "a positive millisecond count")?;
        for cooldown in [None, Some(cooldown)] {
            pretty_assertions::assert_eq!(
                NotificationSlotGate::at(now, UNTHROTTLED, cooldown).notification,
                NotificationStamp::WithOutcome
            );
        }
        let disabled = NotificationCadence::OnChange { throttling_secs: Some(0.0) };
        pretty_assertions::assert_eq!(NotificationSlotGate::at(now, disabled, None).notification, NotificationStamp::WithOutcome);
        Ok(())
    }
}
