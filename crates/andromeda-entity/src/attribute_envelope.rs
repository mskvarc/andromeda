//! Checking that every attribute instance carries the member its type requires
//! (clause 5.2.5).
//!
//! JSON-LD expansion is happy to produce a `Property` with no `hasValue`; the
//! specification is not, and clause 5.5.2 makes that a `BadRequestData` rather
//! than something the store discovers later. The same holds for a member the
//! family does not define at all: clauses 4.5.3, 4.5.18, 4.5.20 and 4.5.24
//! leave four families unitless, so a `unitCode` on one is invalid input
//! (clause 5.5.6) rather than a shape persistence happens to reject.
//!
//! The walk steps over every member NGSI-LD reserves for itself. Those members
//! are not attributes and their sub-nodes are not attribute instances: an
//! Entity's two `scope` paths (clause 4.18) and a LanguageProperty's two
//! language entries (clause 4.5.18) are each one member carrying two values,
//! not one attribute carrying two default instances, and clause 4.5.5.1 says
//! nothing about either.

use crate::error::EntityError;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, uri::Uri};
use andromeda_ngsi_ld_model::expanded::{
    dataset_id::get_instance_dataset_id,
    reserved_member::is_reserved_member,
    unit_code_admission::forbidden_unit_code_family,
};
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use serde_json::Value;
use std::collections::HashSet;

/// Reject an expanded entity carrying an attribute instance with no value.
pub(crate) fn validate_attribute_envelopes(expanded: &Value) -> Result<(), EntityError> {
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
                .map_err(|error| EntityError::validation(format!("invalid expanded attribute name '{key}': {error}")))?;
            if is_reserved_member(&attribute_name) {
                continue;
            }
            validate_attr_instances(&attribute_name, value)?;
        }
    }
    Ok(())
}

/// Reject any instance of one attribute that omits the member its type needs.
fn validate_attr_instances(attr_name: &AttributeName, value: &Value) -> Result<(), EntityError> {
    let instances = match value {
        Value::Array(arr) => arr.as_slice(),
        _other => return Ok(()),
    };

    validate_instance_identities(attr_name, instances)?;

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
                    .ok_or_else(|| EntityError::validation("attribute type is not a supported NGSI-LD attribute type"))
            })
            .collect::<Result<_, _>>()?;

        let is_property = types.contains(&AttributeType::Property);
        let is_geo_property = types.contains(&AttributeType::GeoProperty);
        let is_relationship = types.contains(&AttributeType::Relationship);

        if (is_property || is_geo_property) && !obj.contains_key(expanded_properties::VALUE.as_str()) {
            let short_type = if is_geo_property { "GeoProperty" } else { "Property" };
            return Err(EntityError::validation(format!(
                "{} '{}' is missing required field 'value'",
                short_type, attr_name
            )));
        }

        if is_relationship && !obj.contains_key(expanded_properties::OBJECT.as_str()) {
            return Err(EntityError::validation(format!(
                "Relationship '{}' is missing required field 'object'",
                attr_name
            )));
        }

        if let Some(unitless) = forbidden_unit_code_family(instance) {
            return Err(EntityError::validation(format!(
                "{unitless} '{attr_name}' cannot carry 'unitCode': the specification defines this attribute family without one"
            )));
        }
    }
    Ok(())
}

/// Reject an attribute whose instances do not have distinct identities.
///
/// Clause 4.5.5.1: "There can only be one default Attribute instance for an
/// Attribute with a given Attribute name in any request or response", and the
/// named instances are distinguished by their `datasetId` (clause 5.2.5). Two
/// instances sharing an identity name one instance twice, and a store that
/// accepts them keeps only the last while the payload keeps both — so `q`,
/// which reads the projection, would disagree with the representation.
fn validate_instance_identities(attr_name: &AttributeName, instances: &[Value]) -> Result<(), EntityError> {
    let mut seen: HashSet<Option<Uri>> = HashSet::with_capacity(instances.len());
    for instance in instances {
        if !instance.is_object() {
            continue;
        }
        let dataset_id = get_instance_dataset_id(instance);
        if !seen.insert(dataset_id.clone()) {
            // clone: the reported identity outlives the set that detected it.
            return Err(match dataset_id {
                Some(dataset_id) => EntityError::validation(format!("attribute '{attr_name}' repeats the instance with datasetId '{dataset_id}'")),
                None => EntityError::validation(format!("attribute '{attr_name}' carries more than one default instance")),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::attribute_envelope::validate_attribute_envelopes;
    use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
    use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
    use serde_json::{Value, json};

    /// One expanded entity carrying a single instance of `member`.
    fn entity_with(instance: Value) -> Value {
        json!([{ "@id": "urn:ngsi-ld:Device:1", "https://example.org/member": [instance] }])
    }

    /// Clauses 4.5.3, 4.5.18, 4.5.20 and 4.5.24 define these families without a
    /// `unitCode`, so one on them is invalid input rather than a value to store.
    #[test]
    fn a_unit_code_on_a_family_the_specification_leaves_unitless_is_refused() {
        let unitless = [
            (AttributeType::Relationship, expanded_properties::OBJECT.as_str()),
            (AttributeType::LanguageProperty, expanded_properties::LANGUAGE_MAP.as_str()),
            (AttributeType::VocabProperty, expanded_properties::VOCAB.as_str()),
            (AttributeType::JsonProperty, expanded_properties::JSON.as_str()),
            (AttributeType::ListRelationship, expanded_properties::OBJECT_LIST.as_str()),
        ];
        for (attribute_type, value_key) in unitless {
            let instance = json!({
                "@type": [attribute_type.iri().as_str()],
                value_key: [{"@id": "urn:ngsi-ld:Device:target"}],
                expanded_properties::UNIT_CODE.as_str(): [{"@value": "KGM"}],
            });
            let error = validate_attribute_envelopes(&entity_with(instance)).expect_err("the family admits no unitCode");
            assert!(error.to_string().contains("unitCode"), "{attribute_type}: {error}");
        }
    }

    #[test]
    fn a_property_still_carries_its_unit_code() {
        let instance = json!({
            "@type": [AttributeType::Property.iri().as_str()],
            expanded_properties::VALUE.as_str(): [{"@value": 1}],
            expanded_properties::UNIT_CODE.as_str(): [{"@value": "KGM"}],
        });
        validate_attribute_envelopes(&entity_with(instance)).expect("a Property defines a unitCode member");
    }

    #[test]
    fn a_relationship_without_a_unit_code_is_accepted() {
        let instance = json!({
            "@type": [AttributeType::Relationship.iri().as_str()],
            expanded_properties::OBJECT.as_str(): [{"@id": "urn:ngsi-ld:Device:target"}],
        });
        validate_attribute_envelopes(&entity_with(instance)).expect("a Relationship with no unit is well formed");
    }

    /// Clause 4.5.5.1 admits exactly one default instance per attribute name.
    #[test]
    fn two_default_instances_of_one_attribute_are_refused() {
        let property = |value: i64| {
            json!({
                "@type": [AttributeType::Property.iri().as_str()],
                expanded_properties::VALUE.as_str(): [{"@value": value}],
            })
        };
        let payload = json!([{ "@id": "urn:ngsi-ld:Device:1", "https://example.org/member": [property(1), property(9)] }]);
        let error = validate_attribute_envelopes(&payload).expect_err("one default instance per attribute name");
        assert!(error.to_string().contains("default instance"), "{error}");
    }

    /// Two instances of one `datasetId` name the same instance twice, which
    /// clause 5.2.5 makes the discriminator precisely to prevent.
    #[test]
    fn two_instances_sharing_a_dataset_id_are_refused() {
        let property = |value: i64| {
            json!({
                "@type": [AttributeType::Property.iri().as_str()],
                expanded_properties::VALUE.as_str(): [{"@value": value}],
                expanded_properties::DATASET_ID.as_str(): [{"@id": "urn:ngsi-ld:Dataset:a"}],
            })
        };
        let payload = json!([{ "@id": "urn:ngsi-ld:Device:1", "https://example.org/member": [property(1), property(9)] }]);
        let error = validate_attribute_envelopes(&payload).expect_err("one instance per datasetId");
        assert!(error.to_string().contains("urn:ngsi-ld:Dataset:a"), "{error}");
    }

    #[test]
    fn a_default_instance_beside_named_ones_is_accepted() {
        let property = |dataset_id: Option<&str>| {
            let mut instance = json!({
                "@type": [AttributeType::Property.iri().as_str()],
                expanded_properties::VALUE.as_str(): [{"@value": 1}],
            });
            if let Some(dataset_id) = dataset_id {
                instance[expanded_properties::DATASET_ID.as_str()] = json!([{"@id": dataset_id}]);
            }
            instance
        };
        let payload = json!([{
            "@id": "urn:ngsi-ld:Device:1",
            "https://example.org/member": [property(None), property(Some("urn:ngsi-ld:Dataset:a")), property(Some("urn:ngsi-ld:Dataset:b"))]
        }]);
        validate_attribute_envelopes(&payload).expect("distinct instance identities are well formed");
    }

    /// Clause 4.18 lets one Entity declare several scope paths, which expand
    /// into several nodes under the one `scope` member. `scope` is not an
    /// attribute (clause 4.5.4), so those nodes are paths rather than default
    /// instances of one attribute.
    #[test]
    fn an_entity_declaring_several_scope_paths_is_accepted() {
        let payload = json!([{
            "@id": "urn:ngsi-ld:Building:1",
            expanded_properties::SCOPE.as_str(): [{"@value": "/Madrid/Gardens"}, {"@value": "/CompanyA/UnitC"}],
        }]);
        validate_attribute_envelopes(&payload).expect("several scope paths are one member, not two instances");
    }

    /// Clause 4.5.18 gives a LanguageProperty one entry per language, which
    /// expand into several nodes under the one `languageMap` member. An
    /// attribute fragment carrying two languages is one instance, not two.
    #[test]
    fn an_attribute_fragment_declaring_several_languages_is_accepted() {
        let fragment = json!([{
            expanded_properties::LANGUAGE_MAP.as_str(): [
                {"@value": "Grand Place", "@language": "en"},
                {"@value": "Gran Lugar", "@language": "es"}
            ]
        }]);
        validate_attribute_envelopes(&fragment).expect("two languages are one languageMap, not two instances");
    }

    #[test]
    fn a_property_missing_its_value_is_refused() {
        let instance = json!({"@type": [AttributeType::Property.iri().as_str()]});
        let error = validate_attribute_envelopes(&entity_with(instance)).expect_err("clause 5.2.5 requires a value");
        assert!(error.to_string().contains("value"), "{error}");
    }
}
