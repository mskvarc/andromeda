//! Access to the JSON-LD node object hidden inside an expanded payload.

use serde_json::{Map, Value};

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
    use crate::expanded::node::{expanded_object_mut, expanded_object_ref};
    use serde_json::json;

    #[test]
    fn an_expanded_array_payload_resolves_to_its_first_object() {
        let payload = json!([{"@id": "urn:ngsi-ld:Vehicle:1"}, {"@id": "urn:ngsi-ld:Vehicle:2"}]);
        let node = expanded_object_ref(&payload).expect("first node");
        pretty_assertions::assert_eq!(node["@id"], json!("urn:ngsi-ld:Vehicle:1"));
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
