//! The registrations and entities the batch forwarding tests share.

use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use serde_json::{Value, json};

/// An engine that resolves only what is embedded, never the network.
pub(super) fn offline_engine() -> JsonLdEngine {
    JsonLdEngine::builder().allow_remote(false).build().expect("engine builds")
}

/// Parse a fixture identifier.
pub(super) fn uri(raw: &str) -> Identifier {
    raw.parse().expect("valid URI")
}

/// A registration of `mode` covering the entity selector and properties
/// given, advertising `operations`.
pub(super) fn registration(
    id: &str,
    mode: &str,
    entity_type: Option<&str>,
    entity_id: Option<&str>,
    properties: &[&str],
    operations: &[&str],
) -> ContextSourceRegistration {
    let mut selector = json!({});
    if let Some(entity_type) = entity_type {
        selector["type"] = json!(entity_type);
    }
    if let Some(entity_id) = entity_id {
        selector["id"] = json!(entity_id);
    }
    let mut information = json!({});
    if entity_type.is_some() || entity_id.is_some() {
        information["entities"] = json!([selector]);
    }
    if !properties.is_empty() {
        information["propertyNames"] = json!(properties);
    }
    serde_json::from_value(json!({
    "id": id,
    "type": "ContextSourceRegistration",
    "endpoint": "https://peer.example.org",
    "mode": mode,
    "information": [information],
    "operations": operations,
    "status": "ok",
    "createdAt": "2024-01-01T00:00:00Z",
    "modifiedAt": "2024-01-01T00:00:00Z"
    }))
    .expect("valid registration")
}

/// A Vehicle carrying `brandName`, `speed` and `place`, as a client writes it.
pub(super) fn vehicle(id: &str) -> Value {
    json!({"id": id, "type": "Vehicle", "brandName": {"type": "Property", "value": "x"}, "speed": {"type": "Property", "value": 1}, "place": {"type": "Property", "value": "here"}})
}
