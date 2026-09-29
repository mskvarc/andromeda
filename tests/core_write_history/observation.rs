//! The payloads the suite writes: timestamped readings of one Entity.

use andromeda_test_fixtures::context::CORE_CONTEXT;
use serde_json::{Value, json};

/// The instant `minute` minutes into the suite's reference day.
pub fn at(minute: u32) -> String {
    format!("2026-07-01T{:02}:{:02}:00Z", minute / 60, minute % 60)
}

/// A Property instance holding `value`, observed at minute `minute`.
pub fn reading(value: i64, minute: u32) -> Value {
    json!({"type": "Property", "value": value, "observedAt": at(minute)})
}

/// An Entity carrying `attributes` and the core context inline.
pub fn entity(id: &str, entity_type: &str, attributes: Value) -> Value {
    let mut entity = with_context(attributes);
    entity["id"] = json!(id);
    entity["type"] = json!(entity_type);
    entity
}

/// An Entity Fragment carrying `attributes` and the core context inline.
pub fn fragment(attributes: Value) -> Value {
    with_context(attributes)
}

/// `attributes` with the core context added inline.
fn with_context(mut attributes: Value) -> Value {
    attributes["@context"] = json!(CORE_CONTEXT.as_str());
    attributes
}

/// The core context, for the requests that name it in a `Link` header.
pub fn core_context() -> &'static str {
    CORE_CONTEXT.as_str()
}
