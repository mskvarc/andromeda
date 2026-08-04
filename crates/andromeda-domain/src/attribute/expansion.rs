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
/// Numbers that fit neither `i64`, `u64` nor `f64` cannot be produced by
/// `serde_json`'s parser, so the final arm is unreachable in practice and
/// yields null rather than panicking.
pub(crate) fn serde_json_value_to_json_value<V: JsonValue>(value: &Value) -> V {
    match value {
        Value::Null => V::null(),
        Value::Bool(b) => V::bool(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                V::integer(i)
            } else if let Some(u) = n.as_u64() {
                V::unsigned(u)
            } else if let Some(f) = n.as_f64() {
                V::float(f)
            } else {
                V::null()
            }
        }
        Value::String(s) => V::string(s),
        Value::Array(items) => V::array(items.iter().map(serde_json_value_to_json_value::<V>)),
        // clone: `V::object` takes owned keys and `serde_json::Map` only lends them.
        Value::Object(map) => V::object(map.iter().map(|(k, v)| (k.clone(), serde_json_value_to_json_value::<V>(v)))),
    }
}
