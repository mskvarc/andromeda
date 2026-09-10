//! Access to the JSON-LD node object hidden inside an expanded payload.

use serde_json::{Map, Value};
use std::slice::from_ref;

/// Get a mutable reference to the first object in an expanded JSON-LD value.
///
/// Expanded JSON-LD wraps the top-level node in an array: `[{ ... }]`. This
/// helper handles both the array wrapper and bare object forms.
pub fn expanded_object_mut(entity: &mut Value) -> Option<&mut Map<String, Value>> {
    match entity {
        Value::Array(arr) => arr.first_mut().and_then(Value::as_object_mut),
        Value::Object(obj) => Some(obj),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => None,
    }
}

/// Every top-level node of an expanded JSON-LD value.
///
/// Expansion yields an array of node objects, and the write paths hand around
/// both that array and the bare node one entity expands to. Everything that
/// reads a member off an expanded payload reads it through this, so no two
/// validators disagree about how many nodes a payload has.
#[must_use]
pub const fn expanded_nodes(value: &Value) -> &[Value] {
    match value {
        Value::Array(nodes) => nodes.as_slice(),
        other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_)) => from_ref(other),
    }
}

/// Get a shared reference to the first object in an expanded JSON-LD value.
pub fn expanded_object_ref(value: &Value) -> Option<&Map<String, Value>> {
    match value {
        Value::Array(arr) => arr.first().and_then(Value::as_object),
        Value::Object(obj) => Some(obj),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::expanded::node::{expanded_nodes, expanded_object_mut, expanded_object_ref};
    use serde_json::json;
    use std::slice::from_ref;

    #[test]
    fn an_expanded_array_payload_resolves_to_its_first_object() {
        let payload = json!([{"@id": "urn:ngsi-ld:Vehicle:1"}, {"@id": "urn:ngsi-ld:Vehicle:2"}]);
        let node = expanded_object_ref(&payload).expect("first node");
        pretty_assertions::assert_eq!(node["@id"], json!("urn:ngsi-ld:Vehicle:1"));
    }

    /// The node sequence and the first-node view agree on what a payload holds:
    /// the sequence is what a validator walks, and its first element is what a
    /// mutation reads.
    #[test]
    fn the_node_sequence_holds_every_node_of_an_expanded_payload() {
        let payload = json!([{"@id": "urn:ngsi-ld:Vehicle:1"}, {"@id": "urn:ngsi-ld:Vehicle:2"}]);
        pretty_assertions::assert_eq!(expanded_nodes(&payload).len(), 2);
        pretty_assertions::assert_eq!(expanded_nodes(&payload).first(), payload.as_array().and_then(|nodes| nodes.first()));

        let bare = json!({"@id": "urn:ngsi-ld:Vehicle:1"});
        pretty_assertions::assert_eq!(expanded_nodes(&bare), from_ref(&bare));
        pretty_assertions::assert_eq!(expanded_nodes(&json!([])).len(), 0);
    }

    #[test]
    fn an_expanded_object_payload_resolves_to_itself() {
        let payload = json!({"@id": "urn:ngsi-ld:Vehicle:1"});
        let node = expanded_object_ref(&payload).expect("node");
        pretty_assertions::assert_eq!(node["@id"], json!("urn:ngsi-ld:Vehicle:1"));
    }

    #[test]
    fn a_scalar_payload_resolves_to_nothing() {
        for payload in [json!(null), json!(true), json!(42), json!("urn:ngsi-ld:Vehicle:1")] {
            assert!(expanded_object_ref(&payload).is_none(), "{payload}");
        }
    }

    #[test]
    fn an_empty_array_payload_resolves_to_nothing() {
        assert!(expanded_object_ref(&json!([])).is_none());
        assert!(expanded_object_ref(&json!(["not a node"])).is_none());
    }

    #[test]
    fn a_mutable_node_reference_writes_through_the_array_wrapper() {
        let mut payload = json!([{"@id": "urn:ngsi-ld:Vehicle:1"}]);
        let node = expanded_object_mut(&mut payload).expect("first node");
        node.insert("https://example.com/speed".to_string(), json!([{"@value": 10}]));
        pretty_assertions::assert_eq!(payload[0]["https://example.com/speed"], json!([{"@value": 10}]));
    }

    #[test]
    fn a_mutable_scalar_payload_resolves_to_nothing() {
        let mut payload = json!("urn:ngsi-ld:Vehicle:1");
        assert!(expanded_object_mut(&mut payload).is_none());
    }
}
