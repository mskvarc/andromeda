//! Recognition of the NGSI-LD null sentinel used to delete members in a patch.

use andromeda_ngsi_ld_vocabulary::{NULL_IRI, NULL_URI, expanded::properties};
use serde_json::Value;

/// Returns `true` when an expanded first-level member is the bare NGSI-LD Null.
///
/// This is the expansion of `"attribute": "urn:ngsi-ld:null"` in a merge
/// fragment. Clause 5.5.12 makes it remove the complete target member rather
/// than one dataset-partitioned attribute instance.
pub fn member_is_null_sentinel(instance: &Value) -> bool {
    instance
        .as_object()
        .and_then(|object| object.get("@value").or_else(|| object.get("@id")))
        .and_then(Value::as_str)
        .is_some_and(|value| value == NULL_URI.as_str() || value == NULL_IRI.as_str())
}

/// Returns `true` if the expanded attribute instance is a deletion sentinel
/// (`urn:ngsi-ld:null` or its expanded IRI form in hasValue, hasObject, or
/// hasLanguageMap).
#[must_use]
pub fn instance_is_null_sentinel(instance: &Value) -> bool {
    let Some(obj) = instance.as_object() else {
        return false;
    };

    let check_value_null = |key: &str, value_key: &str| -> bool {
        obj.get(key)
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_object())
            .and_then(|o| o.get(value_key))
            .and_then(|v| v.as_str())
            .is_some_and(|s| s == NULL_URI.as_str() || s == NULL_IRI.as_str())
    };

    check_value_null(properties::VALUE.as_str(), "@value")
        || check_value_null(properties::OBJECT.as_str(), "@id")
        || check_value_null(properties::LANGUAGE_MAP.as_str(), "@value")
}

#[cfg(test)]
mod tests {
    use crate::expanded::null_sentinel::{instance_is_null_sentinel, member_is_null_sentinel};
    use serde_json::json;

    #[test]
    fn null_sentinel_has_value() {
        let instance = json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "urn:ngsi-ld:null"}]
        });
        assert!(instance_is_null_sentinel(&instance));
    }

    #[test]
    fn null_sentinel_has_object() {
        let instance = json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Relationship"],
            "https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:ngsi-ld:null"}]
        });
        assert!(instance_is_null_sentinel(&instance));
    }

    #[test]
    fn non_null_is_not_sentinel() {
        let instance = json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 42}]
        });
        assert!(!instance_is_null_sentinel(&instance));
    }

    #[test]
    fn expanded_first_level_null_is_a_member_sentinel() {
        assert!(member_is_null_sentinel(&json!({"@value": "urn:ngsi-ld:null"})));
        assert!(!member_is_null_sentinel(&json!({"@value": "ordinary"})));
    }
}
