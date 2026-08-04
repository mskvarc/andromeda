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
