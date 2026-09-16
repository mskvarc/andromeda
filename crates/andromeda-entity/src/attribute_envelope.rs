//! Checking that every attribute instance carries the member its type requires
//! and none its type forbids (clauses 5.2.5 and 4.5.2 through 4.5.24).
//!
//! JSON-LD expansion is happy to produce a `Property` with no `hasValue`; the
//! specification is not, and clause 5.5.2 makes that a `BadRequestData` rather
//! than something the store discovers later. The same holds for a member the
//! family does not define at all: clauses 4.5.3, 4.5.18, 4.5.20 and 4.5.24
//! leave four families unitless, so a `unitCode` on one is invalid input
//! (clause 5.5.6) rather than a shape persistence happens to reject.
//!
//! The mirror of the missing mandatory member is the member that belongs to a
//! different family. Clauses 4.5.2.2, 4.5.2.3, 4.5.3.2, 4.5.3.3, 4.5.18.2,
//! 4.5.18.3, 4.5.20.2, 4.5.20.3, 4.5.21.2, 4.5.21.3, 4.5.22.2, 4.5.22.3,
//! 4.5.24.2 and 4.5.24.3 each close with a list of members that "shall never"
//! appear on that family, and clause 5.5.4 makes data that is not
//! syntactically correct per the API data type definitions a `BadRequestData`.
//! A Relationship carrying `value` names two families at once: the renderers
//! read the family's own value member and would answer with only one of them,
//! so what a client wrote and what a query reads back would differ. The rule
//! itself is
//! [`andromeda_ngsi_ld_representation::family_member_admission`]; this walk is
//! where it meets a written payload, which is every Core API entity write
//! path.
//!
//! The walk steps over every member NGSI-LD reserves for itself. Those members
//! are not attributes and their sub-nodes are not attribute instances: an
//! Entity's two `scope` paths (clause 4.18) and a LanguageProperty's two
//! language entries (clause 4.5.18) are each one member carrying two values,
//! not one attribute carrying two default instances, and clause 4.5.5.1 says
//! nothing about either.

use crate::{entity_validation_reason::EntityValidationReason, error::EntityError};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, uri::Uri};
use andromeda_ngsi_ld_model::expanded::{
    dataset_id::get_instance_dataset_id,
    reserved_member::is_reserved_member,
    unit_code_admission::forbidden_unit_code_family,
};
use andromeda_ngsi_ld_representation::family_member_admission::{MemberAdmission, cross_family_members, member_admission};
use andromeda_ngsi_ld_vocabulary::{expanded::properties as expanded_properties, expanded_to_compact};
use serde_json::{Map, Value};
use std::{collections::HashSet, slice::from_ref};

/// Reject an expanded entity carrying an attribute instance with no value, or
/// one carrying a member its family may never carry.
pub(crate) fn validate_attribute_envelopes(expanded: &Value) -> Result<(), EntityError> {
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
            let attribute_name = key.parse::<AttributeName>().map_err(|error| {
                EntityError::Validation(EntityValidationReason::ExpandedAttributeName {
                    name: key.clone(),
                    source: error,
                })
            })?;
            if is_reserved_member(&attribute_name) {
                continue;
            }
            validate_attr_instances(&attribute_name, value)?;
        }
    }

    // Run over the payload as a whole rather than per attribute, so the same
    // walk serves an Entity (whose `@type` names no family, leaving its
    // attribute members to the descent) and the bare Attribute Fragment of
    // clauses 5.6.4 and 5.6.19 (which *is* one instance, and whose own members
    // are the ones to check).
    validate_family_members(None, expanded)
}

/// Reject an attribute instance carrying a member that belongs to a different
/// family, here or on any sub-attribute below it.
///
/// Each member is judged against [`member_admission`] and then sorted into the
/// envelope or into a sub-attribute. Clause 4.5.2.2 makes a sub-attribute an
/// attribute in its own right, so the same rule applies to it one level down;
/// an envelope member carries a value, and descending into one would read a
/// Property's value object as if it were an attribute. An instance declaring no
/// family is left unjudged: there is nothing to hold its members against.
fn validate_family_members(attribute: Option<&AttributeName>, node: &Value) -> Result<(), EntityError> {
    let instances = match node {
        Value::Array(instances) => instances.as_slice(),
        other @ (Value::Object(_) | Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)) => from_ref(other),
    };

    for instance in instances {
        let Some(object) = instance.as_object() else {
            continue;
        };
        let family = declared_family(object);
        for (member, sub_node) in object {
            if member.starts_with('@') {
                continue;
            }
            // An attribute name that is not an IRI cannot name a stored
            // attribute at all; the entity walk above reports it.
            let Ok(member_name) = member.parse::<AttributeName>() else {
                continue;
            };
            if let Some(term) = expanded_to_compact(member) {
                reject_forbidden_member(attribute, family, term)?;
                if is_envelope_member(family, &member_name, term) {
                    continue;
                }
            }
            validate_family_members(Some(&member_name), sub_node)?;
        }
    }
    Ok(())
}

/// Whether a member of an instance is part of the NGSI-LD envelope rather than
/// a sub-attribute of it.
///
/// Three tables answer between them, and being named by the core `@context` is
/// not one of them: clause 4.4 declares `location`, `observationSpace` and
/// `operationSpace` as terms too, and those name attributes.
fn is_envelope_member(family: Option<AttributeType>, member: &AttributeName, term: &str) -> bool {
    is_reserved_member(member) || cross_family_members().any(|carrier| carrier == term) || family.is_some_and(|family| family.reserved_fields().contains(&term))
}

/// The family an expanded instance declares, or `None` when it declares none.
///
/// Unlike clause 4.5.2's reading of an absent `type`, no family is assumed
/// here: a fragment that names none carries no claim to hold its members
/// against, and defaulting it to Property would refuse the `languageMap` of a
/// perfectly valid typeless `LanguageProperty` fragment.
fn declared_family(instance: &Map<String, Value>) -> Option<AttributeType> {
    instance
        .get("@type")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .find_map(AttributeType::from_expanded_iri)
}

/// Report the compacted member `term` as invalid on `family`.
fn reject_forbidden_member(attribute: Option<&AttributeName>, family: Option<AttributeType>, term: &str) -> Result<(), EntityError> {
    let Some(family) = family else {
        return Ok(());
    };
    if member_admission(family, term) == MemberAdmission::Admitted {
        return Ok(());
    }
    Err(EntityError::Validation(EntityValidationReason::MemberForbidden {
        attribute: attribute.cloned(),
        family: Some(family),
        term: term.to_string(),
    }))
}

/// Reject any instance of one attribute that omits the member its type needs.
fn validate_attr_instances(attr_name: &AttributeName, value: &Value) -> Result<(), EntityError> {
    let instances = match value {
        Value::Array(arr) => arr.as_slice(),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_) => return Ok(()),
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
                    .ok_or_else(|| EntityError::Validation(EntityValidationReason::UnsupportedAttributeType))
            })
            .collect::<Result<_, _>>()?;

        let is_property = types.contains(&AttributeType::Property);
        let is_geo_property = types.contains(&AttributeType::GeoProperty);
        let is_relationship = types.contains(&AttributeType::Relationship);

        if (is_property || is_geo_property) && !obj.contains_key(expanded_properties::VALUE.as_str()) {
            let attribute_type = if is_geo_property {
                AttributeType::GeoProperty
            } else {
                AttributeType::Property
            };
            return Err(EntityError::Validation(EntityValidationReason::MissingValue {
                attribute_type,
                attribute: attr_name.clone(),
            }));
        }

        if is_relationship && !obj.contains_key(expanded_properties::OBJECT.as_str()) {
            return Err(EntityError::Validation(EntityValidationReason::MissingObject { attribute: attr_name.clone() }));
        }

        if let Some(family) = forbidden_unit_code_family(instance) {
            return Err(EntityError::Validation(EntityValidationReason::UnitCodeForbidden {
                family,
                attribute: attr_name.clone(),
            }));
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
                Some(dataset_id) => EntityError::Validation(EntityValidationReason::RepeatedInstance {
                    attribute: attr_name.clone(),
                    dataset_id,
                }),
                None => EntityError::Validation(EntityValidationReason::MultipleDefaultInstances { attribute: attr_name.clone() }),
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
    fn entity_with(instance: &Value) -> Value {
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
            let error = validate_attribute_envelopes(&entity_with(&instance)).expect_err("the family admits no unitCode");
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
        validate_attribute_envelopes(&entity_with(&instance)).expect("a Property defines a unitCode member");
    }

    #[test]
    fn a_relationship_without_a_unit_code_is_accepted() {
        let instance = json!({
            "@type": [AttributeType::Relationship.iri().as_str()],
            expanded_properties::OBJECT.as_str(): [{"@id": "urn:ngsi-ld:Device:target"}],
        });
        validate_attribute_envelopes(&entity_with(&instance)).expect("a Relationship with no unit is well formed");
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

    /// Clause 4.5.18 gives a `LanguageProperty` one entry per language, which
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

    /// Clause 4.5.3.2: a Relationship "shall never include" `value`, because
    /// `value` defines a Property value.
    #[test]
    fn a_relationship_carrying_a_property_value_is_refused() {
        let instance = json!({
            "@type": [AttributeType::Relationship.iri().as_str()],
            expanded_properties::OBJECT.as_str(): [{"@id": "urn:ngsi-ld:Device:target"}],
            expanded_properties::VALUE.as_str(): [{"@value": 1}],
        });
        let error = validate_attribute_envelopes(&entity_with(&instance)).expect_err("clause 4.5.3.2 prohibits value on a Relationship");
        assert!(error.to_string().contains("value"), "{error}");
    }

    /// Clause 4.5.2.2: a Property "shall never include" `object`, `objectList`,
    /// `languageMap`, `vocab`, `json`, `valueList`, `entity` or `entityList`,
    /// nor any of their output-only spellings.
    #[test]
    fn a_property_carrying_any_other_familys_value_member_is_refused() {
        let foreign = [
            expanded_properties::OBJECT,
            expanded_properties::OBJECT_LIST,
            expanded_properties::LANGUAGE_MAP,
            expanded_properties::VOCAB,
            expanded_properties::JSON,
            expanded_properties::VALUE_LIST,
            expanded_properties::ENTITY,
            expanded_properties::ENTITY_LIST,
            expanded_properties::PREVIOUS_OBJECT,
            expanded_properties::PREVIOUS_OBJECT_LIST,
            expanded_properties::PREVIOUS_LANGUAGE_MAP,
            expanded_properties::PREVIOUS_VOCAB,
            expanded_properties::PREVIOUS_JSON,
            expanded_properties::PREVIOUS_VALUE_LIST,
        ];
        for member in foreign {
            let instance = json!({
                "@type": [AttributeType::Property.iri().as_str()],
                expanded_properties::VALUE.as_str(): [{"@value": 1}],
                member.as_str(): [{"@value": "forged"}],
            });
            validate_attribute_envelopes(&entity_with(&instance)).expect_err(&format!("clause 4.5.2.2 prohibits {member} on a Property"));
        }
    }

    /// Clauses 4.5.18.2, 4.5.20.2, 4.5.21.2 and 4.5.24.2: `value` is the
    /// generalization each of these families specializes, so none of them may
    /// carry it.
    #[test]
    fn the_specialized_property_families_refuse_the_generalized_value_member() {
        let specialized = [
            (AttributeType::LanguageProperty, expanded_properties::LANGUAGE_MAP),
            (AttributeType::VocabProperty, expanded_properties::VOCAB),
            (AttributeType::JsonProperty, expanded_properties::JSON),
            (AttributeType::ListProperty, expanded_properties::VALUE_LIST),
        ];
        for (attribute_type, own_member) in specialized {
            let instance = json!({
                "@type": [attribute_type.iri().as_str()],
                own_member.as_str(): [{"@value": "kept"}],
                expanded_properties::VALUE.as_str(): [{"@value": 1}],
            });
            let error = validate_attribute_envelopes(&entity_with(&instance)).expect_err("value generalizes this family's own member");
            assert!(error.to_string().contains("value"), "{attribute_type}: {error}");
        }
    }

    /// Clause 4.5.22.2: `object` generalizes `objectList`, so a
    /// `ListRelationship` may not carry it.
    #[test]
    fn a_list_relationship_carrying_the_generalized_object_is_refused() {
        let instance = json!({
            "@type": [AttributeType::ListRelationship.iri().as_str()],
            expanded_properties::OBJECT_LIST.as_str(): [{"@list": [{"@id": "urn:ngsi-ld:Device:a"}]}],
            expanded_properties::OBJECT.as_str(): [{"@id": "urn:ngsi-ld:Device:b"}],
        });
        let error = validate_attribute_envelopes(&entity_with(&instance)).expect_err("clause 4.5.22.2 prohibits object on a ListRelationship");
        assert!(error.to_string().contains("object"), "{error}");
    }

    /// A sub-attribute is an attribute (clause 4.5.2.2), so the same
    /// prohibition reaches it.
    #[test]
    fn a_sub_attribute_carrying_another_familys_value_member_is_refused() {
        let instance = json!({
            "@type": [AttributeType::Property.iri().as_str()],
            expanded_properties::VALUE.as_str(): [{"@value": 1}],
            "https://example.org/accuracy": [{
                "@type": [AttributeType::Relationship.iri().as_str()],
                expanded_properties::OBJECT.as_str(): [{"@id": "urn:ngsi-ld:Device:target"}],
                expanded_properties::LANGUAGE_MAP.as_str(): [{"@value": "forged", "@language": "en"}],
            }],
        });
        let error = validate_attribute_envelopes(&entity_with(&instance)).expect_err("clause 4.5.3.2 prohibits languageMap on a Relationship");
        assert!(error.to_string().contains("accuracy"), "the offending sub-attribute is named: {error}");
    }

    /// Clause 4.4 declares `location` as a core term, and clause 4.7 makes it
    /// the default `GeoProperty` of an Entity: an attribute, not part of the
    /// attribute envelope, so the walk reaches its instances.
    #[test]
    fn an_attribute_named_by_a_core_term_is_still_walked_as_an_attribute() {
        let payload = json!([{
            "@id": "urn:ngsi-ld:Device:1",
            expanded_properties::LOCATION.as_str(): [{
                "@type": [AttributeType::GeoProperty.iri().as_str()],
                expanded_properties::VALUE.as_str(): [{"@value": "{\"type\":\"Point\",\"coordinates\":[1.0,2.0]}"}],
                expanded_properties::OBJECT.as_str(): [{"@id": "urn:ngsi-ld:Device:target"}],
            }],
        }]);
        let error = validate_attribute_envelopes(&payload).expect_err("clause 4.5.2.2 prohibits object on a GeoProperty");
        assert!(error.to_string().contains("object"), "{error}");
    }

    /// Each family keeps the member it declares for itself, in both the
    /// current and the output-only spelling.
    #[test]
    fn every_family_still_accepts_its_own_value_member() {
        for attribute_type in AttributeType::ALL {
            let instance = json!({
                "@type": [attribute_type.iri().as_str()],
                attribute_type.value_key_str(): [{"@value": "kept"}],
            });
            validate_attribute_envelopes(&entity_with(&instance)).expect("a family declares its own value member");
        }
    }

    #[test]
    fn a_property_missing_its_value_is_refused() {
        let instance = json!({"@type": [AttributeType::Property.iri().as_str()]});
        let error = validate_attribute_envelopes(&entity_with(&instance)).expect_err("clause 5.2.5 requires a value");
        assert!(error.to_string().contains("value"), "{error}");
    }
}
