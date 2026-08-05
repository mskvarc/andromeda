//! Recognition of the NGSI-LD null sentinel used to delete members in a patch.

use crate::vocab::expanded::properties;
use serde_json::Value;

/// Returns `true` if the expanded attribute instance is a deletion sentinel
/// (`urn:ngsi-ld:null` or its expanded IRI form in hasValue, hasObject, or
/// hasLanguageMap).
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
            .is_some_and(|s| s == properties::NULL_URI.as_str() || s == properties::NULL_IRI.as_str())
    };

    check_value_null(properties::VALUE.as_str(), "@value")
        || check_value_null(properties::OBJECT.as_str(), "@id")
        || check_value_null(properties::LANGUAGE_MAP.as_str(), "@value")
}

#[cfg(test)]
mod tests {
    use crate::expanded::null_sentinel::instance_is_null_sentinel;
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
}
