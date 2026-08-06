//! Percent-decoding of the URIs a Relationship (and ListRelationship) points at.
//!
//! An entity identifier is a URI, and a client is free to percent-encode
//! characters in one when it writes a Relationship. The store keeps what it was
//! given, so the decoding happens at the presentation boundary. It runs before
//! any GeoJSON wrapping, over the plain entity, so the policy does not depend on
//! the negotiated media type — a GeoJSON Feature carries the same decoded URIs
//! its JSON counterpart does.

use andromeda_ngsi_ld_vocabulary::compact::{classes as compact_classes, properties as compact_properties};
use serde_json::Value;

/// Decode percent-encoded URIs in Relationship `object` and ListRelationship
/// `objectList` fields.
///
/// Postgres stores URIs as-is, but some clients percent-encode characters in
/// entity IDs (e.g. colons). This function decodes them for human-readable
/// response output, walking a Relationship's single `object` and a
/// ListRelationship's ordered `objectList`.
///
/// A pipeline stage of [`crate::response::ResponseRenderer`], run over the plain
/// entity before any GeoJSON envelope is built.
pub(crate) fn decode_percent_encoded_uris(value: &mut Value) {
    match value {
        Value::Object(obj) => {
            // Read the discriminant out as booleans before taking a mutable
            // borrow of the value member, so the immutable borrow of `type` does
            // not outlive into the `get_mut` below.
            let attr_type = obj.get("type").and_then(Value::as_str);
            let is_relationship = attr_type == Some(compact_classes::RELATIONSHIP);
            let is_list_relationship = attr_type == Some(compact_classes::LIST_RELATIONSHIP);

            if is_relationship && let Some(Value::String(uri)) = obj.get_mut(compact_properties::OBJECT) {
                decode_uri_in_place(uri);
            }
            if is_list_relationship && let Some(Value::Array(objects)) = obj.get_mut(compact_properties::OBJECT_LIST) {
                for object in objects {
                    if let Value::String(uri) = object {
                        decode_uri_in_place(uri);
                    }
                }
            }
            for v in obj.values_mut() {
                decode_percent_encoded_uris(v);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                decode_percent_encoded_uris(v);
            }
        }
        // Scalars have no URIs to decode
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

/// Decode a single percent-encoded URI in place, leaving it unchanged when it
/// carries no `%` or does not decode as UTF-8.
fn decode_uri_in_place(uri: &mut String) {
    if uri.contains('%')
        && let Ok(decoded) = percent_encoding::percent_decode_str(uri).decode_utf8()
    {
        *uri = decoded.into_owned();
    }
}

#[cfg(test)]
mod tests {
    use crate::uri_decoding::decode_percent_encoded_uris;
    use serde_json::json;

    #[test]
    fn decode_percent_encoded_relationship_object() {
        let mut value = json!({
            "id": "urn:x", "type": "T",
            "ref": {"type": "Relationship", "object": "urn%3Angsi-ld%3ABuilding%3A1"}
        });
        decode_percent_encoded_uris(&mut value);
        pretty_assertions::assert_eq!(value["ref"]["object"], "urn:ngsi-ld:Building:1");
    }

    #[test]
    fn decode_percent_encoded_uris_skips_non_relationships() {
        let mut value = json!({"id": "urn:x", "type": "T", "name": {"type": "Property", "value": "test%20value"}});
        let original = value.clone();
        decode_percent_encoded_uris(&mut value);
        pretty_assertions::assert_eq!(value, original);
    }

    #[test]
    fn decode_percent_encoded_uris_recursive_in_arrays() {
        let mut value = json!([
            {"type": "Relationship", "object": "urn%3Ax"},
            {"type": "Relationship", "object": "urn:y"}
        ]);
        decode_percent_encoded_uris(&mut value);
        pretty_assertions::assert_eq!(value[0]["object"], "urn:x");
        pretty_assertions::assert_eq!(value[1]["object"], "urn:y");
    }
}
