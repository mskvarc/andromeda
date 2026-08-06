//! Reading the parts of one expanded NGSI-LD attribute instance.
//!
//! The temporal write path stores an instance's `datasetId`, `observedAt` and
//! `instanceId` in dedicated columns and everything else verbatim as metadata.
//! This module owns which key is which.

use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use iri_rs::IriBuf;
use serde_json::{Map, Value};
use uuid::Uuid;

/// Whether an expanded JSON-LD object declares one of the NGSI-LD attribute types.
///
/// Anything else in an entity payload is a plain JSON-LD node and carries no
/// temporal evolution, so it is skipped rather than recorded.
pub fn is_ngsi_ld_attribute_instance(obj: &Map<String, Value>) -> bool {
    let Some(types) = obj.get("@type").and_then(|v| v.as_array()) else {
        return false;
    };
    types.iter().filter_map(|t| t.as_str()).any(|s| AttributeType::from_expanded_iri(s).is_some())
}

/// The instance's NGSI-LD attribute type, defaulting to `Property`.
///
/// Clause 4.5.2 makes `Property` the type of an attribute that declares none.
pub fn detect_attribute_type(instance: &Value) -> AttributeType {
    let empty: Vec<Value> = vec![];
    let types = instance.get("@type").and_then(|v| v.as_array()).unwrap_or(&empty);

    for t in types {
        if let Some(s) = t.as_str()
            && let Some(attr_type) = AttributeType::from_expanded_iri(s)
        {
            return attr_type;
        }
    }
    AttributeType::Property
}

/// The instance's `observedAt` in its lexical RFC 3339 form, if it declares one.
pub fn extract_observed_at(instance: &Value) -> Option<String> {
    instance
        .get(expanded_properties::OBSERVED_AT.as_str())
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|obj| obj.get("@value"))
        .and_then(|v| v.as_str())
        .map(String::from)
}

/// Extract an attribute instance's `datasetId` (NGSI-LD 5.2.5).
///
/// Typed as a URI, not a URN: the specification constrains `datasetId` to a
/// URI. Parsing it as a URN discarded every plain-URI dataset identifier,
/// which silently merged distinct instances onto the default one and produced
/// a wrong `attribute_current` grain.
pub fn extract_dataset_id(instance: &Value) -> Option<IriBuf> {
    instance
        .get(expanded_properties::DATASET_ID.as_str())
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|obj| obj.get("@id"))
        .and_then(|v| v.as_str())
        .and_then(|value| IriBuf::new(value.to_owned()).ok())
}

/// The instance's `instanceId`, and whether the client supplied it.
///
/// Clause 4.5.7 makes `instanceId` the identity of a temporal instance, so a
/// client-supplied one must survive; a generated one is the broker's own and
/// may be replaced on a rewrite.
pub fn extract_or_generate_instance_id(instance: &Value) -> (String, bool) {
    instance
        .get(expanded_properties::INSTANCE_ID.as_str())
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|obj| obj.get("@id"))
        .and_then(|v| v.as_str())
        .map_or_else(|| (generated_instance_id(), false), |s| (s.to_owned(), true))
}

/// Mint an `instanceId` for an instance that arrived without one.
pub fn generated_instance_id() -> String {
    format!("urn:ngsi-ld:attribute:instance:{}", Uuid::new_v4())
}

/// Mint an `instanceId` for a recorded scope observation.
pub fn generated_scope_instance_id() -> String {
    format!("urn:ngsi-ld:scope:instance:{}", Uuid::new_v4())
}

/// Whether the key is stored in a dedicated column instead of the metadata blob.
fn is_metadata_skip_key(key: &str) -> bool {
    key == "@id"
        || key == "@type"
        || key == expanded_properties::DATASET_ID.as_str()
        || key == expanded_properties::JSON.as_str()
        || key == expanded_properties::LANGUAGE_MAP.as_str()
        || key == expanded_properties::OBJECT.as_str()
        || key == expanded_properties::VALUE.as_str()
        || key == expanded_properties::VOCAB.as_str()
        || key == expanded_properties::INSTANCE_ID.as_str()
        || key == expanded_properties::OBSERVED_AT.as_str()
}

/// Everything on the instance that has no dedicated column, kept verbatim.
pub fn collect_metadata(instance: &Value) -> Option<Value> {
    let obj = instance.as_object()?;

    let metadata: Map<String, Value> = obj
        .iter()
        .filter(|(k, _)| !is_metadata_skip_key(k))
        .map(|(k, v)| (k.clone(), v.clone())) // clone: owned key-value pairs for the new metadata Map
        .collect();

    if metadata.is_empty() { None } else { Some(Value::Object(metadata)) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_model::expanded::instance_key::is_system_attribute;

    #[test]
    fn metadata_skip_key_matches_known_values() {
        assert!(is_metadata_skip_key("@id"));
        assert!(is_metadata_skip_key(expanded_properties::DATASET_ID.as_str()));
        assert!(is_metadata_skip_key(expanded_properties::VALUE.as_str()));
        assert!(!is_metadata_skip_key("https://example.com/custom"));
    }

    #[test]
    fn system_attribute_matches_known_values() {
        assert!(is_system_attribute(expanded_properties::CREATED_AT.as_str()));
        assert!(is_system_attribute(expanded_properties::UNIT_CODE.as_str()));
        assert!(!is_system_attribute("https://example.com/custom"));
    }

    #[test]
    fn extract_instance_id_flags_client_supplied_when_present() {
        let instance = serde_json::json!({
            expanded_properties::INSTANCE_ID.as_str(): [
                {"@id": "urn:ngsi-ld:attribute:instance:provided"}
            ]
        });
        let (id, supplied) = extract_or_generate_instance_id(&instance);
        pretty_assertions::assert_eq!(id, "urn:ngsi-ld:attribute:instance:provided");
        assert!(supplied);
    }

    #[test]
    fn extract_instance_id_flags_generated_when_absent() {
        let instance = serde_json::json!({});
        let (id, supplied) = extract_or_generate_instance_id(&instance);
        assert!(id.starts_with("urn:ngsi-ld:attribute:instance:"));
        assert!(!supplied);
    }

    #[test]
    fn dataset_id_accepts_a_plain_uri() {
        // NGSI-LD 5.2.5 types datasetId as a URI. Parsing it as a URN discarded
        // every non-URN identifier, merging distinct instances onto the default
        // one and giving attribute_current the wrong grain.
        let instance = serde_json::json!({
            expanded_properties::DATASET_ID.as_str(): [{"@id": "http://example.org/d1"}]
        });
        pretty_assertions::assert_eq!(extract_dataset_id(&instance).as_ref().map(IriBuf::as_str), Some("http://example.org/d1"));
    }

    #[test]
    fn dataset_id_still_accepts_a_urn() {
        let instance = serde_json::json!({
            expanded_properties::DATASET_ID.as_str(): [{"@id": "urn:ngsi-ld:Dataset:d1"}]
        });
        pretty_assertions::assert_eq!(extract_dataset_id(&instance).as_ref().map(IriBuf::as_str), Some("urn:ngsi-ld:Dataset:d1"));
    }

    #[test]
    fn dataset_id_is_absent_when_the_instance_has_none() {
        assert!(extract_dataset_id(&serde_json::json!({})).is_none());
    }

    #[test]
    fn an_object_with_no_ngsi_ld_type_is_not_an_attribute_instance() {
        let plain = serde_json::json!({"@type": ["https://example.com/Thing"]});
        assert!(!is_ngsi_ld_attribute_instance(plain.as_object().expect("object")));
        let untyped = serde_json::json!({});
        assert!(!is_ngsi_ld_attribute_instance(untyped.as_object().expect("object")));
    }

    #[test]
    fn an_instance_with_no_declared_type_defaults_to_property() {
        pretty_assertions::assert_eq!(detect_attribute_type(&serde_json::json!({})), AttributeType::Property);
    }
}
