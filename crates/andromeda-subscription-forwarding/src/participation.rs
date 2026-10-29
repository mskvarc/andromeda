//! Whether a local subscription holds a companion and forwarded copies.
//!
//! A `localOnly` subscription never takes part in distributed operations
//! (clause 5.8.1.4). An expired one no longer does: clause 5.8.1.4 stops its
//! notifications once `expiresAt` is due, so a copy at a context source would
//! only notify a subscription that delivers nothing. Its status is read as a
//! read reports it, so a subscription past its `expiresAt` is out of
//! distributed operations before the expiry sweep has persisted the transition.

use andromeda_ngsi_ld_model::subscription::{Subscription, SubscriptionStatus};
use chrono::{DateTime, Utc};

/// Whether `subscription` stays out of distributed operations at `at`.
pub(crate) fn stays_local(subscription: &Subscription, at: DateTime<Utc>) -> bool {
    subscription.delivery.local_only == Some(true) || subscription.lifecycle.standing.reported_status(at) == SubscriptionStatus::Expired
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::participation::stays_local;
    use andromeda_ngsi_ld_model::subscription::Subscription;
    use chrono::{DateTime, Utc};
    use serde_json::{Value, json};

    fn at() -> DateTime<Utc> {
        "2026-06-01T00:00:00Z".parse().expect("valid instant")
    }

    fn subscription(members: Value) -> Subscription {
        let mut document = json!({
            "id": "urn:ngsi-ld:Subscription:local",
            "type": "Subscription",
            "entities": [{"type": "https://example.org/Vehicle"}],
            "notification": {"endpoint": {"uri": "http://subscriber/notify"}},
            "status": "active",
            "createdAt": "2026-01-01T00:00:00Z",
            "modifiedAt": "2026-01-01T00:00:00Z"
        });
        if let (Some(document), Value::Object(members)) = (document.as_object_mut(), members) {
            document.extend(members);
        }
        serde_json::from_value(document).expect("valid subscription")
    }

    #[test]
    fn an_active_subscription_before_its_expiration_takes_part() {
        assert!(!stays_local(&subscription(json!({})), at()));
        assert!(!stays_local(&subscription(json!({"expiresAt": "2026-06-01T00:00:01Z"})), at()));
        assert!(!stays_local(&subscription(json!({"status": "paused"})), at()));
    }

    #[test]
    fn a_local_only_subscription_stays_local() {
        assert!(stays_local(&subscription(json!({"localOnly": true})), at()));
    }

    /// Clause 5.8.1.4: the status is `expired` once `expiresAt` is due,
    /// whether or not the sweep has persisted it.
    #[test]
    fn an_expired_subscription_stays_local() {
        assert!(stays_local(&subscription(json!({"status": "expired"})), at()));
        assert!(stays_local(&subscription(json!({"expiresAt": "2026-06-01T00:00:00Z"})), at()));
        assert!(stays_local(
            &subscription(json!({"status": "paused", "expiresAt": "2026-05-31T00:00:00Z"})),
            at()
        ));
    }
}
