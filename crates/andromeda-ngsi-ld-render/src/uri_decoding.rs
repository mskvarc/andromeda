//! Percent-decoding of the URIs a Relationship points at.
//!
//! An entity identifier is a URI, and a client is free to percent-encode
//! characters in one when it writes a Relationship. The store keeps what it
//! was given, so the decoding happens at the presentation boundary — on the
//! JSON and JSON-LD renderings, and not on the GeoJSON one, where the
//! Relationship sits inside a Feature envelope this never walks into.

use serde_json::Value;

/// Decode percent-encoded URIs in Relationship `object` fields.
///
/// Postgres stores URIs as-is, but some clients percent-encode characters in
/// entity IDs (e.g. colons). This function decodes them for human-readable
/// response output. Applied to JSON/LD+JSON responses but not GeoJSON (which
/// wraps the entity in a Feature envelope).
///
/// A pipeline stage of [`crate::response::ResponseRenderer`], which decides
/// whether it runs from the media type and the payload shape.
pub(crate) fn decode_percent_encoded_uris(value: &mut Value) {
    match value {
        Value::Object(obj) => {
            if obj.get("type").and_then(|t| t.as_str()) == Some("Relationship")
                && let Some(Value::String(uri)) = obj.get_mut("object")
                && uri.contains('%')
                && let Ok(decoded) = percent_encoding::percent_decode_str(uri).decode_utf8()
            {
                *uri = decoded.into_owned();
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
