//! A compiled periodic subscription and the tick it is evaluated at, for the
//! tests of a periodic notification's read.

use andromeda_context_commit::{store_instant::StoreInstant, subscription_key::SubscriptionKey};
use andromeda_ngsi_ld_query::filter::language_matching::LanguageMatching;
use andromeda_subscription::{
    compilation::compile_subscription,
    compiled::CompiledSubscription,
    notifying_since::NotifyingSince,
    stored_subscription::StoredSubscription,
};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};

/// The Vehicle type the fixtures select.
pub(crate) const VEHICLE: &str = "https://example.org/Vehicle";

/// A periodic subscription with `members` beside its notification, compiled.
pub(crate) fn periodic(members: &Value) -> CompiledSubscription {
    let mut subscription = json!({
        "id": "urn:ngsi-ld:Subscription:fleet",
        "type": "Subscription",
        "timeInterval": 60,
        "notification": {"endpoint": {"uri": "http://subscriber.example.org/notify", "accept": "application/json"}},
        "status": "active",
        "createdAt": "2024-01-01T00:00:00Z",
        "modifiedAt": "2024-01-01T00:00:00Z"
    });
    for (member, value) in members.as_object().expect("members are an object") {
        subscription[member] = value.clone();
    }
    compile_subscription(
        &StoredSubscription {
            key: SubscriptionKey::try_from(1).expect("a valid key"),
            subscription: serde_json::from_value(subscription).expect("valid subscription"),
            notifying_since: NotifyingSince {
                active: StoreInstant::new(DateTime::<Utc>::MIN_UTC),
                selected: StoreInstant::new(DateTime::<Utc>::MIN_UTC),
            },
        },
        LanguageMatching::TagEquality,
    )
}

/// The instant a tick is evaluated at.
pub(crate) fn tick() -> DateTime<Utc> {
    DateTime::<Utc>::from_timestamp(1_893_456_000, 0).expect("a valid instant")
}
