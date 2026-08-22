//! The two normalisations a compacted discovery document needs before it is a
//! clause 5.2.24 to 5.2.28 resource.

use serde_json::Value;

/// Strip the `ngsi-ld:` prefix from compacted JSON keys.
///
/// The JSON-LD compactor sometimes produces CURIE forms like `ngsi-ld:attributeDetails`
/// instead of the bare short name `attributeDetails`, depending on how the context
/// defines the term (simple alias vs. expanded definition with `@container`).
/// The NGSI-LD API spec expects short names, so we normalize these in a post-pass.
pub(super) fn strip_ngsild_prefix(value: &mut Value) {
    match value {
        Value::Object(obj) => {
            let keys: Vec<String> = obj.keys().cloned().collect(); // clone: owned keys to avoid borrowing obj during mutation below
            for key in keys {
                if let Some(short) = key.strip_prefix("ngsi-ld:")
                    && let Some(val) = obj.remove(&key)
                {
                    obj.insert(short.to_string(), val);
                }
            }
            for val in obj.values_mut() {
                strip_ngsild_prefix(val);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                strip_ngsild_prefix(item);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

/// Ensure specified fields are always arrays in compacted output.
/// JSON-LD compaction collapses single-element arrays to scalars, but several
/// NGSI-LD discovery members are defined as `String[]` (e.g. `attributeTypes`,
/// `typeNames`; clauses 5.2.24 to 5.2.28) and must stay arrays.
pub(super) fn ensure_array_fields(value: &mut Value, fields: &[&str]) {
    match value {
        Value::Object(obj) => {
            for &field in fields {
                if let Some(val) = obj.get_mut(field)
                    && !val.is_array()
                {
                    *val = serde_json::json!([val]);
                }
            }
            // Recurse into nested arrays (attributeDetails, @graph, etc.)
            for val in obj.values_mut() {
                if val.is_array() || val.is_object() {
                    ensure_array_fields(val, fields);
                }
            }
        }
        Value::Array(arr) => {
            for item in arr {
                ensure_array_fields(item, fields);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_ngsi_ld_prefix_is_stripped_from_keys_at_every_depth() {
        let mut value = json!({"ngsi-ld:attributeDetails": [{"ngsi-ld:attributeName": "speed"}], "typeName": "Vehicle"});
        strip_ngsild_prefix(&mut value);
        pretty_assertions::assert_eq!(value, json!({"attributeDetails": [{"attributeName": "speed"}], "typeName": "Vehicle"}));
    }

    #[test]
    fn a_collapsed_scalar_member_is_restored_to_an_array() {
        let mut value = json!({"attributeDetails": [{"attributeTypes": "Property"}], "typeNames": "Vehicle"});
        ensure_array_fields(&mut value, &["attributeTypes", "typeNames"]);
        pretty_assertions::assert_eq!(value, json!({"attributeDetails": [{"attributeTypes": ["Property"]}], "typeNames": ["Vehicle"]}));
    }
}
