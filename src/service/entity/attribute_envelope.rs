//! Checking that every attribute instance carries the member its type requires
//! (clause 5.2.5).
//!
//! JSON-LD expansion is happy to produce a `Property` with no `hasValue`; the
//! specification is not, and clause 5.5.2 makes that a `BadRequestData` rather
//! than something the store discovers later.

use crate::core::error::AndromedaError;
use andromeda_ngsi_ld_vocabulary::expanded::{classes as expanded_classes, properties as expanded_properties};
use serde_json::Value;

/// Reject an expanded entity carrying an attribute instance with no value.
pub(crate) fn validate_attribute_envelopes(expanded: &Value) -> Result<(), AndromedaError> {
    let entities = match expanded {
        Value::Array(arr) => arr.as_slice(),
        _other => std::slice::from_ref(expanded),
    };

    for entity in entities {
        let Some(obj) = entity.as_object() else {
            continue;
        };
        for (key, value) in obj {
            if key.starts_with('@') {
                continue;
            }
            validate_attr_instances(key, value)?;
        }
    }
    Ok(())
}

/// Reject any instance of one attribute that omits the member its type needs.
fn validate_attr_instances(attr_name: &str, value: &Value) -> Result<(), AndromedaError> {
    let instances = match value {
        Value::Array(arr) => arr.as_slice(),
        _other => return Ok(()),
    };

    for instance in instances {
        let Some(obj) = instance.as_object() else {
            continue;
        };

        let types: Vec<&str> = obj
            .get("@type")
            .and_then(|t| t.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();

        let is_property = types.iter().any(|t| *t == expanded_classes::PROPERTY.as_str());
        let is_geo_property = types.iter().any(|t| *t == expanded_classes::GEO_PROPERTY.as_str());
        let is_relationship = types.iter().any(|t| *t == expanded_classes::RELATIONSHIP.as_str());

        if (is_property || is_geo_property) && !obj.contains_key(expanded_properties::VALUE.as_str()) {
            let short_type = if is_geo_property { "GeoProperty" } else { "Property" };
            return Err(AndromedaError::entity_validation(format!(
                "{} '{}' is missing required field 'value'",
                short_type, attr_name
            )));
        }

        if is_relationship && !obj.contains_key(expanded_properties::OBJECT.as_str()) {
            return Err(AndromedaError::entity_validation(format!(
                "Relationship '{}' is missing required field 'object'",
                attr_name
            )));
        }
    }
    Ok(())
}
