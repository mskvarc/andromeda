//! Whether a subscription notifies at an instant, and the status a read reports
//! for it (ETSI GS CIM 009 Table 5.2.12-2 and clause 5.8.1.4).
//!
//! Every reader that asks either question, the change matcher, both caches,
//! the periodic sweeps and the representation, asks this type, so the
//! `expiresAt` boundary has one reading.

use crate::subscription::SubscriptionStatus;
use andromeda_ngsi_ld_lexicon::time::instant_serde;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The `status` and `expiresAt` members of a subscription.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionStanding {
    /// The lifecycle status the subscription was last given.
    pub status: SubscriptionStatus,
    /// The instant the subscription expires at; `None` never expires.
    #[serde(skip_serializing_if = "Option::is_none", with = "instant_serde::optional", default)]
    pub expires_at: Option<DateTime<Utc>>,
}

impl SubscriptionStanding {
    /// Whether the subscription is notified at `at`: its status is `active`
    /// and its `expiresAt` has not been reached (clause 5.8.1.4).
    #[must_use]
    pub fn notifies_at(&self, at: DateTime<Utc>) -> bool {
        self.status == SubscriptionStatus::Active && SubscriptionStanding::unexpired_at(self.expires_at, at)
    }

    /// The status a read reports at `at`: `expired` once `expiresAt` has been
    /// reached, whatever status the subscription was last given, and that
    /// status otherwise.
    ///
    /// Clause 5.8.1.4 has the status change "automatically" when the
    /// expiration date is due, so a read between that instant and the sweep
    /// persisting the transition reports the status already reached.
    #[must_use]
    pub fn reported_status(&self, at: DateTime<Utc>) -> SubscriptionStatus {
        if SubscriptionStanding::unexpired_at(self.expires_at, at) {
            self.status
        } else {
            SubscriptionStatus::Expired
        }
    }

    /// Whether an `expiresAt` of `expires_at` still lies after `at`; a
    /// subscription without one never expires.
    ///
    /// The instant itself is expired, the boundary the store's own expiry
    /// predicate applies.
    #[must_use]
    pub fn unexpired_at(expires_at: Option<DateTime<Utc>>, at: DateTime<Utc>) -> bool {
        expires_at.is_none_or(|expires_at| expires_at > at)
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::subscription::{SubscriptionStatus, standing::SubscriptionStanding};
    use chrono::{DateTime, Utc};

    fn at(unix_seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(unix_seconds, 0).expect("a fixture timestamp in range")
    }

    const fn standing(status: SubscriptionStatus, expires_at: Option<DateTime<Utc>>) -> SubscriptionStanding {
        SubscriptionStanding { status, expires_at }
    }

    /// Clause 5.8.1.4: only an active subscription before its `expiresAt` is
    /// notified, and the expiration instant itself is past.
    #[test]
    fn standing_notifies_only_while_active_and_before_expires_at() {
        assert!(standing(SubscriptionStatus::Active, None).notifies_at(at(1_000)));
        assert!(standing(SubscriptionStatus::Active, Some(at(2_000))).notifies_at(at(1_999)));
        assert!(!standing(SubscriptionStatus::Active, Some(at(2_000))).notifies_at(at(2_000)));
        assert!(!standing(SubscriptionStatus::Active, Some(at(2_000))).notifies_at(at(2_001)));
        assert!(!standing(SubscriptionStatus::Paused, None).notifies_at(at(1_000)));
        assert!(!standing(SubscriptionStatus::Expired, None).notifies_at(at(1_000)));
    }

    /// Table 5.2.12-2: `expired` once `expiresAt` has passed, whatever status
    /// the row still carries; the stored status otherwise.
    #[test]
    fn reported_status_is_expired_once_expires_at_passed_and_paused_otherwise_kept() {
        assert_eq!(
            standing(SubscriptionStatus::Active, Some(at(2_000))).reported_status(at(2_000)),
            SubscriptionStatus::Expired
        );
        assert_eq!(
            standing(SubscriptionStatus::Paused, Some(at(2_000))).reported_status(at(2_001)),
            SubscriptionStatus::Expired
        );
        assert_eq!(
            standing(SubscriptionStatus::Paused, Some(at(2_000))).reported_status(at(1_999)),
            SubscriptionStatus::Paused
        );
        assert_eq!(
            standing(SubscriptionStatus::Paused, None).reported_status(at(1_000)),
            SubscriptionStatus::Paused
        );
        assert_eq!(
            standing(SubscriptionStatus::Active, None).reported_status(at(1_000)),
            SubscriptionStatus::Active
        );
    }
}
