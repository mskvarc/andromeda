//! Temporal-entity payload builders for the provision forwarding tests.
//!
//! Temporal provision operations carry attribute *instances* (arrays of nodes
//! with `observedAt`) rather than the single nodes of the current-entity API, so
//! these builders produce a temporal `Vehicle`, an add/modify fragment, and the
//! single instance node used by instance-scoped operations.

use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use serde_json::{Value, json};

/// A temporal `Vehicle` holding one `brandName` and one `speed` instance, with
/// the compound context inline for `application/ld+json` submission.
pub fn temporal_vehicle(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "brandName": [ { "type": "Property", "value": "Mercedes", "observedAt": "2021-01-01T00:00:00Z" } ],
        "speed": [ { "type": "Property", "value": 42, "observedAt": "2021-01-01T00:00:00Z" } ],
        "@context": COMPOUND_CONTEXT.as_str()
    })
}

/// A temporal fragment adding a single `speed` instance, for add-attributes.
pub fn temporal_speed_fragment() -> Value {
    json!({
        "speed": [ { "type": "Property", "value": 99, "observedAt": "2021-02-01T00:00:00Z" } ],
        "@context": COMPOUND_CONTEXT.as_str()
    })
}

/// The `EntityTemporal` Fragment a modification of one `speed` instance carries.
pub fn speed_instance() -> Value {
    // Clause 5.6.14.3: an EntityTemporal Fragment naming `speed`, its one
    // instance contained by an Array of exactly one item.
    json!({
        "speed": [{"type": "Property", "value": 100, "observedAt": "2021-03-01T00:00:00Z"}],
        "@context": COMPOUND_CONTEXT.as_str()
    })
}
