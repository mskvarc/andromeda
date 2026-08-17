//! JSON-LD value-expansion helpers shared by the attribute families.
//!
//! These translate Andromeda's own representations into the generic
//! [`JsonValue`] abstraction the `jsonld` expander writes through, so no
//! attribute module has to know how a node reference or a plain JSON scalar is
//! encoded in expanded form (ETSI GS CIM 009 clause 4.6.2).

use jsonld_expandable_core::JsonValue;
use serde_json::Value;
use std::iter::once;

/// Build an expanded JSON-LD node reference `{"@id": s}`.
pub(crate) fn id_object<V: JsonValue>(s: &str) -> V {
    V::object(once(("@id".to_string(), V::string(s))))
}

/// Convert a [`serde_json::Value`] into the expander's [`JsonValue`] representation.
///
/// The generic expansion trait cannot construct an arbitrary-precision number.
/// Such a number is carried as its exact text in this internal scaffold and is
/// replaced with a native `serde_json::Number` by the exact expansion interface
/// before the expanded document leaves the model.
pub(crate) fn serde_json_value_to_json_value<V: JsonValue>(value: &Value) -> V {
    match value {
        Value::Null => V::null(),
        Value::Bool(b) => V::bool(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                V::integer(i)
            } else if let Some(u) = n.as_u64() {
                V::unsigned(u)
            } else {
                V::string(n.as_str())
            }
        }
        Value::String(s) => V::string(s),
        Value::Array(items) => V::array(items.iter().map(serde_json_value_to_json_value::<V>)),
        // clone: `V::object` takes owned keys and `serde_json::Map` only lends them.
        Value::Object(map) => V::object(map.iter().map(|(k, v)| (k.clone(), serde_json_value_to_json_value::<V>(v)))),
    }
}
