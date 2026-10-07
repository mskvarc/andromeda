//! The comparison the typed-model round-trip tests share: a resource read back is the resource that was written, once the members the broker owns are set aside.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.2.2 makes the members a client supplies ones the broker stores and serves back, and Tables 5.2.12-1, 5.2.9-2 and 5.2.14.2-1 name the read-only members the broker adds itself (`createdAt`, `modifiedAt`, `status`, `isActive`, the delivery counters and timestamps). Those are system-generated, so a round trip ignores them and compares everything else as parsed JSON.

use serde_json::Value;

/// `jsonldContext` is here because clause 5.8.1.4 has the broker initialise it from "the @context applicable for the Subscription" when the client omits it, so the value read back is the broker's choice of spelling.
const SYSTEM_MEMBERS: [&str; 11] = [
    "@context",
    "jsonldContext",
    "createdAt",
    "modifiedAt",
    "status",
    "isActive",
    "state",
    "timesSent",
    "timesFailed",
    "lastSuccess",
    "lastFailure",
];

/// The members the broker generates inside a Subscription's `notification`
/// (Table 5.2.14.2-1).
const NOTIFICATION_SYSTEM_MEMBERS: [&str; 6] = ["status", "timesSent", "timesFailed", "lastNotification", "lastSuccess", "lastFailure"];

/// `resource` with every broker-generated member removed.
pub fn without_system_members(mut resource: Value) -> Value {
    if let Some(members) = resource.as_object_mut() {
        for member in SYSTEM_MEMBERS {
            members.remove(member);
        }
        if let Some(notification) = members.get_mut("notification").and_then(Value::as_object_mut) {
            for member in NOTIFICATION_SYSTEM_MEMBERS {
                notification.remove(member);
            }
        }
    }
    resource
}

/// Assert that `returned` is `posted` apart from the broker-generated members.
pub fn assert_round_trips(posted: Value, returned: Value, resource: &str) {
    pretty_assertions::assert_eq!(
        without_system_members(returned),
        without_system_members(posted),
        "the {resource} read back differs from the {resource} written"
    );
}

#[cfg(test)]
mod tests {
    use super::{assert_round_trips, without_system_members};
    use serde_json::json;

    #[test]
    fn round_trip_comparison_removes_top_level_and_notification_system_members() {
        let stripped = without_system_members(json!({
            "id": "urn:ngsi-ld:Subscription:a",
            "@context": ["https://example.org/context.jsonld"],
            "createdAt": "2025-01-01T00:00:00Z",
            "status": "active",
            "notification": {"format": "normalized", "timesSent": 2, "status": "ok"}
        }));

        pretty_assertions::assert_eq!(stripped, json!({"id": "urn:ngsi-ld:Subscription:a", "notification": {"format": "normalized"}}));
    }

    #[test]
    fn round_trip_comparison_accepts_a_resource_that_only_gained_system_members() {
        assert_round_trips(
            json!({"id": "urn:ngsi-ld:Subscription:a", "type": "Subscription"}),
            json!({"id": "urn:ngsi-ld:Subscription:a", "type": "Subscription", "modifiedAt": "2025-01-01T00:00:00Z"}),
            "Subscription",
        );
    }
}
