//! The stored forms of the two subscriptions a scene holds: the local one and
//! the companion mirroring it.

use crate::reassembly_harness::{companion_subscription, default_iri, local_subscription};
use serde_json::{Value, json};

/// The stored form of the companion: it mirrors the selection of the local
/// subscription as the default members describe it.
pub(crate) fn stored_companion() -> Value {
    json!({
        "id": companion_subscription().as_str(),
        "type": "Subscription",
        "entities": [{"type": default_iri("Vehicle")}],
        "notification": {"endpoint": {"uri": "http://broker.example:1056/ngsi-ld/andromeda/v1/notifications"}},
        "status": "active",
        "createdAt": "2020-01-01T00:00:00Z",
        "modifiedAt": "2020-01-01T00:00:00Z"
    })
}

/// The stored form of the local subscription: it watches `Vehicle`, notifies
/// `endpoint` in the normalized format, and takes `members` over. The
/// endpoint is always the receiver's, whatever `members` say about it.
pub(crate) fn stored_subscription(members: &Value, endpoint: &str) -> Value {
    let mut stored = json!({
        "id": local_subscription().as_str(),
        "type": "Subscription",
        "entities": [{"type": default_iri("Vehicle")}],
        "notification": {"format": "normalized"},
        "status": "active",
        "createdAt": "2020-01-01T00:00:00Z",
        "modifiedAt": "2020-01-01T00:00:00Z"
    });
    if let (Some(base), Some(replacements)) = (stored.as_object_mut(), members.as_object()) {
        base.extend(replacements.iter().map(|(name, value)| (name.clone(), value.clone())));
    }
    stored["notification"]["endpoint"] = json!({"uri": endpoint});
    stored
}
