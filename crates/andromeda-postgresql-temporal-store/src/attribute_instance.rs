//! Reading the parts of one expanded NGSI-LD attribute instance.
//!
//! The temporal instance-modification path of clause 5.6.15 stores an
//! instance's `observedAt` in a dedicated column and everything else verbatim
//! as metadata. This module owns which key is which.

use crate::error::TemporalStoreError;
use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, ngsi_ld_date_time::NgsiLdDateTime};
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use chrono::{DateTime, Utc};
use serde_json::{Map, Value};

/// The instance's NGSI-LD attribute type, defaulting to `Property`.
///
/// Clause 4.5.2 makes `Property` the type of an attribute that declares none.
#[must_use]
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

/// The instance's validated `observedAt`, if it declares one.
///
/// Parsing is routed through [`NgsiLdDateTime`] so `observedAt` is held to the
/// same clause 4.6.3 strict-UTC, six-fractional-digit policy as every other
/// client-supplied `DateTime`, rather than accepting whatever `chrono`'s bare
/// RFC 3339 parser admits.
///
/// # Errors
///
/// Returns [`TemporalStoreError::TimestampPrecision`] when the instance
/// declares an `observedAt` whose `@value` is not a clause 4.6.3 `DateTime`:
/// not RFC 3339, not expressed in UTC, or carrying a sub-microsecond fraction
/// the store cannot hold.
pub fn extract_observed_at(instance: &Value) -> Result<Option<DateTime<Utc>>, TemporalStoreError> {
    instance
        .get(expanded_properties::OBSERVED_AT.as_str())
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|obj| obj.get("@value"))
        .and_then(|v| v.as_str())
        .map(|raw| raw.parse::<NgsiLdDateTime>().map(NgsiLdDateTime::get).map_err(TemporalStoreError::from))
        .transpose()
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
#[must_use]
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
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_model::expanded::instance_key::is_system_attribute;

    fn attribute_name(value: &str) -> AttributeName {
        value.parse().expect("attribute name")
    }

    #[test]
    fn metadata_skip_key_matches_known_values() {
        assert!(is_metadata_skip_key("@id"));
        assert!(is_metadata_skip_key(expanded_properties::DATASET_ID.as_str()));
        assert!(is_metadata_skip_key(expanded_properties::VALUE.as_str()));
        assert!(!is_metadata_skip_key("https://example.com/custom"));
    }

    #[test]
    fn system_attribute_matches_known_values() {
        assert!(is_system_attribute(&attribute_name(expanded_properties::CREATED_AT.as_str())));
        assert!(is_system_attribute(&attribute_name(expanded_properties::UNIT_CODE.as_str())));
        assert!(!is_system_attribute(&attribute_name("https://example.com/custom")));
    }

    #[test]
    fn malformed_observed_at_is_rejected() {
        let instance = serde_json::json!({
            expanded_properties::OBSERVED_AT.as_str(): [{"@value": "not-a-timestamp"}]
        });
        assert!(extract_observed_at(&instance).is_err());
    }

    #[test]
    fn an_instance_with_no_declared_type_defaults_to_property() {
        pretty_assertions::assert_eq!(detect_attribute_type(&serde_json::json!({})), AttributeType::Property);
    }
}
