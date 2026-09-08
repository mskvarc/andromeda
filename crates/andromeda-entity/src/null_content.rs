//! Rejecting the `urn:ngsi-ld:null` deletion sentinel where the spec forbids it.
//!
//! Clause 5.5.4 gives `urn:ngsi-ld:null` meaning only inside the fragment of a
//! Merge (clause 5.6.17) or Partial Update Attribute (clause 5.6.4), where it
//! deletes a member. On the non-fragment write paths — Create (clause 5.6.1),
//! Replace Entity (clause 5.6.18, whose clause 5.6.18.4 states Replace does not
//! support nulls), Replace Attribute (clause 5.6.19) and Append (clause 5.6.3)
//! — a `urn:ngsi-ld:null` is invalid input and must be a `BadRequestData` rather
//! than a stored value.
//!
//! The check runs on the expanded payload, beside
//! [`validate_attribute_envelopes`](crate::attribute_envelope::validate_attribute_envelopes),
//! and looks only at first-level attribute members: a bare member set to the
//! sentinel, a Property `value` set to it, or a Relationship `object` set to it.
//! Nested members of a Property value object are left alone, since the fragment
//! paths that read them are exempt.

use crate::error::EntityError;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::expanded::{
    null_sentinel::{instance_is_null_sentinel, member_is_null_sentinel},
    reserved_member::is_reserved_member,
};
use serde_json::Value;
use std::slice::from_ref;

/// Reject an expanded write whose first-level attribute members carry the
/// `urn:ngsi-ld:null` deletion sentinel (clause 5.5.4).
pub(crate) fn reject_null_content(expanded: &Value) -> Result<(), EntityError> {
    let entities = match expanded {
        Value::Array(arr) => arr.as_slice(),
        other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_)) => from_ref(other),
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
                .map_err(|error| EntityError::validation(format!("invalid expanded attribute name '{key}': {error}")))?;
            if is_reserved_member(&attribute_name) {
                continue;
            }
            let Some(instances) = value.as_array() else {
                continue;
            };
            for instance in instances {
                if member_is_null_sentinel(instance) || instance_is_null_sentinel(instance) {
                    return Err(EntityError::validation(format!(
                        "attribute '{attribute_name}' carries the reserved value 'urn:ngsi-ld:null', which this operation does not permit (clause 5.5.4)"
                    )));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::null_content::reject_null_content;
    use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
    use serde_json::{Value, json};

    /// One expanded entity carrying a single `member` attribute value.
    fn entity_with(member: &Value) -> Value {
        json!([{ "@id": "urn:ngsi-ld:Device:1", "https://example.org/member": member }])
    }

    #[test]
    fn a_property_value_of_the_null_sentinel_is_rejected() {
        let member = json!([{
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            expanded_properties::VALUE.as_str(): [{"@value": "urn:ngsi-ld:null"}]
        }]);
        let error = reject_null_content(&entity_with(&member)).expect_err("clause 5.5.4 forbids a null value here");
        assert!(error.to_string().contains("urn:ngsi-ld:null"), "{error}");
    }

    #[test]
    fn a_relationship_object_of_the_null_sentinel_is_rejected() {
        let member = json!([{
            "@type": ["https://uri.etsi.org/ngsi-ld/Relationship"],
            expanded_properties::OBJECT.as_str(): [{"@id": "urn:ngsi-ld:null"}]
        }]);
        let error = reject_null_content(&entity_with(&member)).expect_err("clause 5.5.4 forbids a null object here");
        assert!(error.to_string().contains("urn:ngsi-ld:null"), "{error}");
    }

    #[test]
    fn a_bare_first_level_null_member_is_rejected() {
        let member = json!([{"@value": "urn:ngsi-ld:null"}]);
        let error = reject_null_content(&entity_with(&member)).expect_err("a bare null member is forbidden");
        assert!(error.to_string().contains("urn:ngsi-ld:null"), "{error}");
    }

    #[test]
    fn an_ordinary_property_value_is_accepted() {
        let member = json!([{
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            expanded_properties::VALUE.as_str(): [{"@value": "office"}]
        }]);
        reject_null_content(&entity_with(&member)).expect("an ordinary value is well formed");
    }
}
