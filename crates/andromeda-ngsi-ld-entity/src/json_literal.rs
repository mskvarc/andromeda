//! An opaque JSON value (`{"@type": "@json", "@value": ..}`).
//!
//! Clause 4.5.24 keeps the value of a `JsonProperty` "not available for
//! JSON-LD interpretation", and a context may coerce any Property term to
//! `@json` too. This is the one place a generic JSON tree is the domain value.

use andromeda_ngsi_ld_vocabulary::NULL_URI;
use derive_more::{AsRef, From, Into};
use serde_json::Value;

/// A native JSON document carried verbatim.
#[derive(AsRef, Clone, Debug, Eq, From, Into, PartialEq)]
pub struct JsonLiteral(Value);

impl JsonLiteral {
    /// Carry `value` verbatim.
    #[must_use]
    pub const fn new(value: Value) -> JsonLiteral {
        JsonLiteral(value)
    }

    /// The JSON document.
    #[must_use]
    pub const fn get(&self) -> &Value {
        &self.0
    }

    /// Give the JSON document up.
    #[must_use]
    pub fn into_inner(self) -> Value {
        self.0
    }

    /// Whether the document is itself the NGSI-LD Null string: the
    /// deletion encoding of clause 4.5.24.2.
    #[must_use]
    pub fn is_null_sentinel(&self) -> bool {
        is_null_string(&self.0)
    }

    /// Whether some key of the document, at any depth and through arrays,
    /// holds the NGSI-LD Null string.
    ///
    /// That is the clause 5.5.4 third prohibition, which only a merge fragment
    /// is exempt from: there the key is deleted (clause 5.5.12, RFC 7396). The
    /// document being the sentinel itself is the second prohibition, not this
    /// one. A JSON `null` is data, never the sentinel.
    #[must_use]
    pub fn carries_null_sentinel_under_a_key(&self) -> bool {
        carries_under_a_key(&self.0)
    }
}

/// Whether `value` is the NGSI-LD Null string.
///
/// Only the `urn:ngsi-ld:null` spelling counts: expansion never descends into a
/// `@json` value, so nothing there was ever expanded into the IRI spelling.
fn is_null_string(value: &Value) -> bool {
    value.as_str() == Some(NULL_URI.as_str())
}

/// Whether some key below `value` holds the NGSI-LD Null string.
fn carries_under_a_key(value: &Value) -> bool {
    match value {
        Value::Object(members) => members.values().any(is_or_contains_null_string),
        Value::Array(items) => items.iter().any(carries_under_a_key),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => false,
    }
}

/// Whether a value held under some key is the NGSI-LD Null string, or nests one.
fn is_or_contains_null_string(value: &Value) -> bool {
    match value {
        Value::String(_) => is_null_string(value),
        Value::Array(items) => items.iter().any(is_or_contains_null_string),
        Value::Object(members) => members.values().any(is_or_contains_null_string),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use crate::json_literal::JsonLiteral;
    use serde_json::json;

    #[test]
    fn a_sentinel_under_some_key_is_found_at_any_depth() {
        assert!(JsonLiteral::new(json!({"a": {"b": [{"c": "urn:ngsi-ld:null"}]}})).carries_null_sentinel_under_a_key());
        assert!(!JsonLiteral::new(json!({"a": null})).carries_null_sentinel_under_a_key());
    }

    #[test]
    fn the_whole_value_being_the_sentinel_is_not_the_nested_prohibition() {
        let literal = JsonLiteral::new(json!("urn:ngsi-ld:null"));
        assert!(literal.is_null_sentinel());
        assert!(!literal.carries_null_sentinel_under_a_key());
    }
}
