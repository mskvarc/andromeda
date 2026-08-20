//! Subscription document construction shared across the MAP subscription and
//! notification cases (clause 5.2.12). A base subscription watches `Vehicle`
//! entities and delivers a change-driven, normalized notification to the given
//! endpoint; tests mutate one member to isolate a single behaviour.

use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use serde_json::{Value, json};

/// A minimal valid change-driven subscription over `Vehicle` entities that
/// delivers `application/json` normalized notifications to `notify_uri`.
pub fn subscription(id: &str, notify_uri: &str) -> Value {
    json!({
        "id": id,
        "type": "Subscription",
        "entities": [ { "type": "Vehicle" } ],
        "notification": {
            "format": "normalized",
            "endpoint": { "uri": notify_uri, "accept": "application/json" }
        },
        "@context": COMPOUND_CONTEXT.as_str()
    })
}

/// Constrain the watched entity type of a subscription in place.
pub fn set_entity_type(subscription: &mut Value, entity_type: &str) {
    subscription["entities"] = json!([ { "type": entity_type } ]);
}

/// Set the subscription's `watchedAttributes` array in place.
pub fn set_watched_attributes(subscription: &mut Value, attributes: &[&str]) {
    subscription["watchedAttributes"] = json!(attributes);
}

/// Set the subscription's `notification.attributes` projection in place.
pub fn set_notification_attributes(subscription: &mut Value, attributes: &[&str]) {
    subscription["notification"]["attributes"] = json!(attributes);
}
