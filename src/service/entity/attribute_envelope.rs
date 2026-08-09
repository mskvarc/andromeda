//! Checking that every attribute instance carries the member its type requires
//! (clause 5.2.5).
//!
//! JSON-LD expansion is happy to produce a `Property` with no `hasValue`; the
//! specification is not, and clause 5.5.2 makes that a `BadRequestData` rather
//! than something the store discovers later.

use crate::error::AndromedaError;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType};
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
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
            let attribute_name = key
                .parse::<AttributeName>()
                .map_err(|error| AndromedaError::entity_validation(format!("invalid expanded attribute name '{key}': {error}")))?;
            validate_attr_instances(&attribute_name, value)?;
        }
    }
    Ok(())
}

/// Reject any instance of one attribute that omits the member its type needs.
fn validate_attr_instances(attr_name: &AttributeName, value: &Value) -> Result<(), AndromedaError> {
    let instances = match value {
        Value::Array(arr) => arr.as_slice(),
        _other => return Ok(()),
    };

    for instance in instances {
        let Some(obj) = instance.as_object() else {
            continue;
        };

        let types: Vec<AttributeType> = obj
            .get("@type")
            .and_then(|t| t.as_array())
            .into_iter()
            .flatten()
            .map(|value| {
                value
                    .as_str()
                    .and_then(AttributeType::from_expanded_iri)
                    .ok_or_else(|| AndromedaError::entity_validation("attribute type is not a supported NGSI-LD attribute type"))
            })
            .collect::<Result<_, _>>()?;

        let is_property = types.contains(&AttributeType::Property);
        let is_geo_property = types.contains(&AttributeType::GeoProperty);
        let is_relationship = types.contains(&AttributeType::Relationship);

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
