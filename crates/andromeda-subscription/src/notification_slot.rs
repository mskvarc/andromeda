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
use std::time::Duration;

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
    /// Never: the claim only reads whether a `throttling` window and the
    /// endpoint's `cooldown` would admit a notification, and moves nothing,
    /// so a notification that is not going to be sent takes no slot. A
    /// `ceiling` of `None` is an unthrottled subscription.
    Unstamped {
        /// Latest `last_notification` that still permits a claim.
        ceiling: Option<DateTime<Utc>>,
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

impl NotificationSlotGate {
    /// This gate with its stamping removed: it refuses exactly what `self`
    /// refuses and takes no slot.
    ///
    /// Used to ask whether a notification that cannot be built would have
    /// been sent, so that a failure is recorded only for a notification the
    /// subscription was entitled to.
    #[must_use]
    pub const fn unstamped(self) -> NotificationSlotGate {
        NotificationSlotGate {
            notification: NotificationStamp::Unstamped {
                ceiling: self.throttling_ceiling(),
            },
            last_failure: self.last_failure,
        }
    }

    /// The windows that refused a claim of this gate, taken at `now`, judged
    /// from the `stamps` the store held when it refused.
    ///
    /// A window refused the claim when its stamp is past the gate's ceiling,
    /// and it reopens once the stamp has fallen back to the ceiling: `stamp -
    /// ceiling` after `now`. The store reads the stamps in the statement that
    /// refused, but a claim racing it can move a stamp after that read, so
    /// stamps that explain no refusal are answered with the gate's whole
    /// window, the longest the refusing stamp can still need.
    #[must_use]
    pub fn withholding(self, now: DateTime<Utc>, stamps: DeliveryStamps) -> Withholding {
        let refusing = |ceiling: Option<DateTime<Utc>>, stamp: Option<DateTime<Utc>>| match (ceiling, stamp) {
            (Some(ceiling), Some(stamp)) if stamp > ceiling => Some((stamp, stamp - ceiling)),
            (Some(_) | None, Some(_) | None) => None,
        };
        let throttling = refusing(self.throttling_ceiling(), stamps.last_notification);
        let cooldown = refusing(self.last_failure, stamps.last_failure);
        let explained = [throttling, cooldown].into_iter().flatten().filter_map(|(_, wait)| wait.to_std().ok()).max();
        Withholding {
            throttled_since: throttling.map(|(stamp, _)| stamp),
            cooled_since: cooldown.map(|(stamp, _)| stamp),
            reopens_after: explained.unwrap_or_else(|| self.reopens_after(now)),
        }
    }

    /// How long after `now`, the instant the gate was taken at, a claim it
    /// refused is admitted again however recent the refusing stamp: the
    /// longer of the subscription's `throttling` and its endpoint's
    /// `cooldown`, zero for a gate with neither.
    ///
    /// Every stamp the gate compares is at or before `now`, so once this has
    /// passed, neither window still covers the stamp that refused the claim.
    fn reopens_after(self, now: DateTime<Utc>) -> Duration {
        [self.throttling_ceiling(), self.last_failure]
            .into_iter()
            .flatten()
            .filter_map(|ceiling| (now - ceiling).to_std().ok())
            .max()
            .unwrap_or_default()
    }

    /// Latest `last_notification` that still permits a claim (clause 5.2.12
    /// `throttling`); `None` when the subscription is not throttled.
    const fn throttling_ceiling(self) -> Option<DateTime<Utc>> {
        match self.notification {
            NotificationStamp::Throttled { ceiling } | NotificationStamp::Unstamped { ceiling: Some(ceiling) } => Some(ceiling),
            NotificationStamp::WithOutcome | NotificationStamp::Periodic { .. } | NotificationStamp::Unstamped { ceiling: None } => None,
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
/// `throttling` bounds the same claim from the other side. A refusal carries
/// the stamps the windows were measured from, so a queued notification can
/// tell a window that was open when its change was made from one opened since.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationSlot {
    /// The slot was taken and the notification is the caller's to send; its
    /// outcome is recorded with the [`OutcomeRecord`] carried.
    Claimed(OutcomeRecord),
    /// No notification is sent at all and the delivery instants did not
    /// move, for the reason carried.
    Suppressed(Suppression),
}

/// Why a notification-slot claim took no slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Suppression {
    /// The subscription is no longer active, or its branch is gone.
    Inactive,
    /// A `throttling` or `cooldown` window refused the claim; the stamps are
    /// the ones the store held when it refused.
    Windowed(DeliveryStamps),
}

/// The stored instants a subscription's notification windows are measured
/// from (clause 5.2.12 `throttling` and Table 5.2.15-1 `cooldown`), on the
/// store's clock.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DeliveryStamps {
    /// The instant of the last notification sent (`lastNotification`),
    /// `None` when none was.
    pub last_notification: Option<DateTime<Utc>>,
    /// The instant of the last failed notification (`lastFailure`), `None`
    /// when none failed.
    pub last_failure: Option<DateTime<Utc>>,
}

/// The windows that withheld one notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Withholding {
    /// The `lastNotification` whose `throttling` window refused the claim,
    /// `None` when that window did not.
    pub throttled_since: Option<DateTime<Utc>>,
    /// The `lastFailure` whose `cooldown` refused the claim, `None` when the
    /// cooldown did not.
    pub cooled_since: Option<DateTime<Utc>>,
    /// How long after the claim both refusing windows have passed.
    pub reopens_after: Duration,
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
    use crate::notification_slot::{DeliveryStamps, NotificationCadence, NotificationSlotGate, NotificationStamp, Withholding};
    use andromeda_ngsi_ld_lexicon::positive_milliseconds::PositiveMilliseconds;
    use chrono::{TimeDelta, TimeZone, Utc};
    use std::time::Duration;

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
                ceiling: now - TimeDelta::seconds(5)
            }
        );
        assert!(throttled.last_failure.is_none());

        let cooled = NotificationSlotGate::at(now, UNTHROTTLED, Some(cooldown));
        pretty_assertions::assert_eq!(cooled.notification, NotificationStamp::WithOutcome);
        pretty_assertions::assert_eq!(cooled.last_failure, Some(now - TimeDelta::milliseconds(2_000)));

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

    /// A refused claim is admitted again once the longer window has passed.
    #[test]
    fn a_refused_claim_reopens_after_the_longer_of_the_throttling_and_the_cooldown() -> Result<(), &'static str> {
        let now = Utc.timestamp_opt(1_000, 0).single().ok_or("the fixture timestamp must be valid")?;
        let cooldown = PositiveMilliseconds::try_from(2_500).map_err(|_error| "a positive millisecond count")?;
        let throttling = NotificationCadence::OnChange { throttling_secs: Some(5.0) };
        for (gate, window) in [
            (NotificationSlotGate::at(now, UNTHROTTLED, None), Duration::ZERO),
            (NotificationSlotGate::at(now, throttling, None), Duration::from_secs(5)),
            (NotificationSlotGate::at(now, UNTHROTTLED, Some(cooldown)), Duration::from_millis(2_500)),
            (NotificationSlotGate::at(now, throttling, Some(cooldown)), Duration::from_secs(5)),
            (NotificationSlotGate::at(now, throttling, Some(cooldown)).unstamped(), Duration::from_secs(5)),
            (NotificationSlotGate::at(now, NotificationCadence::Periodic, None), Duration::ZERO),
        ] {
            pretty_assertions::assert_eq!(gate.reopens_after(now), window, "{gate:?}");
        }
        Ok(())
    }

    /// A refusal names the stamp of each window that covers the claim, and
    /// is admitted again once the latest of them falls out of its window.
    #[test]
    fn a_refusal_names_each_window_its_stamps_fall_inside_and_when_the_last_one_passes() -> Result<(), &'static str> {
        let now = Utc.timestamp_opt(1_000, 0).single().ok_or("the fixture timestamp must be valid")?;
        let cooldown = PositiveMilliseconds::try_from(4_000).map_err(|_error| "a positive millisecond count")?;
        let throttling = NotificationCadence::OnChange { throttling_secs: Some(10.0) };
        let gate = NotificationSlotGate::at(now, throttling, Some(cooldown));
        let notified = now - TimeDelta::seconds(7);
        let failed = now - TimeDelta::seconds(1);
        let stamps = |last_notification, last_failure| DeliveryStamps {
            last_notification,
            last_failure,
        };
        pretty_assertions::assert_eq!(
            gate.withholding(now, stamps(Some(notified), None)),
            Withholding {
                throttled_since: Some(notified),
                cooled_since: None,
                reopens_after: Duration::from_secs(3),
            }
        );
        pretty_assertions::assert_eq!(
            gate.withholding(now, stamps(None, Some(failed))),
            Withholding {
                throttled_since: None,
                cooled_since: Some(failed),
                reopens_after: Duration::from_secs(3),
            }
        );
        pretty_assertions::assert_eq!(
            gate.withholding(now, stamps(Some(now - TimeDelta::seconds(2)), Some(failed))),
            Withholding {
                throttled_since: Some(now - TimeDelta::seconds(2)),
                cooled_since: Some(failed),
                reopens_after: Duration::from_secs(8),
            }
        );
        Ok(())
    }

    /// A stamp counts only against a window the gate has: an unthrottled
    /// subscription that notified a moment ago is refused by its cooldown
    /// alone.
    #[test]
    fn a_stamp_counts_only_against_a_window_the_gate_has() -> Result<(), &'static str> {
        let now = Utc.timestamp_opt(1_000, 0).single().ok_or("the fixture timestamp must be valid")?;
        let cooldown = PositiveMilliseconds::try_from(4_000).map_err(|_error| "a positive millisecond count")?;
        let failed = now - TimeDelta::seconds(1);
        let withholding = NotificationSlotGate::at(now, UNTHROTTLED, Some(cooldown)).withholding(
            now,
            DeliveryStamps {
                last_notification: Some(now),
                last_failure: Some(failed),
            },
        );
        pretty_assertions::assert_eq!(withholding.throttled_since, None);
        pretty_assertions::assert_eq!(withholding.cooled_since, Some(failed));
        Ok(())
    }

    /// A claim racing the refusal can move a stamp after the store read it,
    /// so stamps no window covers wait the gate's whole window rather than
    /// none.
    #[test]
    fn stamps_that_explain_no_refusal_wait_the_whole_window() -> Result<(), &'static str> {
        let now = Utc.timestamp_opt(1_000, 0).single().ok_or("the fixture timestamp must be valid")?;
        let throttling = NotificationCadence::OnChange { throttling_secs: Some(10.0) };
        let withholding = NotificationSlotGate::at(now, throttling, None).withholding(
            now,
            DeliveryStamps {
                last_notification: Some(now - TimeDelta::seconds(30)),
                last_failure: None,
            },
        );
        pretty_assertions::assert_eq!(
            withholding,
            Withholding {
                throttled_since: None,
                cooled_since: None,
                reopens_after: Duration::from_secs(10),
            }
        );
        Ok(())
    }

    /// Removing the stamping keeps the ceilings, so the probe refuses what the
    /// claim would have refused.
    #[test]
    fn an_unstamped_gate_keeps_both_ceilings_and_stamps_nothing() -> Result<(), &'static str> {
        let now = Utc.timestamp_opt(1_000, 0).single().ok_or("the fixture timestamp must be valid")?;
        let cooldown = PositiveMilliseconds::try_from(2_000).map_err(|_error| "a positive millisecond count")?;
        let throttling = NotificationCadence::OnChange { throttling_secs: Some(5.0) };

        let gate = NotificationSlotGate::at(now, throttling, Some(cooldown)).unstamped();
        pretty_assertions::assert_eq!(
            gate.notification,
            NotificationStamp::Unstamped {
                ceiling: Some(now - TimeDelta::seconds(5))
            }
        );
        pretty_assertions::assert_eq!(gate.last_failure, Some(now - TimeDelta::milliseconds(2_000)));

        pretty_assertions::assert_eq!(
            NotificationSlotGate::at(now, UNTHROTTLED, None).unstamped().notification,
            NotificationStamp::Unstamped { ceiling: None }
        );
        Ok(())
    }
}
