//! The two clause 5.5.4 rules decidable only on the compact payload, read off
//! the typed compact entity.
//!
//! [`validate_fragment_attribute_types`] and [`screen_keys_in_fragment`] state
//! the rules over the compact JSON of an entity: clause 5.2.4's eight attribute
//! types together with clause 4.5.2.2's `ngsildproof` seals, and the keys
//! clause 4.6.4 refuses because JSON-LD expansion would drop or rewrite them.
//! Clause 5.5.7's `@context` below the top level is not among them: it is part
//! of the request's user `@context`, refused by the user `@context` admission
//! (`andromeda_jsonld::user_context_admission`) before this screening runs. The extractor decodes the request straight
//! into the typed [`Entity`] or [`EntityPatch`], so the rules are applied here
//! to what that decode holds, member by member, rather than to a JSON copy of
//! the whole entity rendered back for them:
//!
//! - a member the decode carries verbatim, and every client-written JSON value
//!   inside a typed instance, is read by the representation crate's own walk,
//!   its location reported from the top of the entity;
//! - a typed instance declares one of the eight types by construction, holds
//!   any seal it was written with as a sub-attribute, and is keyed by its
//!   family's members and by attribute names, so it can carry a refused key
//!   only inside a JSON value or a sub-attribute, and only those are read;
//! - a typed geometry is RFC 7946 positions and nothing else, so neither rule
//!   has anything to read in it, which is what keeps a large geometry out of
//!   the screening.
//!
//! Each rule reports the first fault in the member order of the compact JSON
//! it is stated over, and the attribute-type rule is decided over every member
//! before the key rule, exactly as the two run over a rendered entity.
//!
//! [`validate_fragment_attribute_types`]: andromeda_ngsi_ld_representation::attribute_type_validation::validate_fragment_attribute_types
//! [`screen_keys_in_fragment`]: andromeda_ngsi_ld_representation::compact_key_admission::screen_keys_in_fragment

use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::{
    attribute::{
        inline_entity::InlineEntity,
        instance::{Attribute, AttributeValue},
        one_or_many::OneOrMany,
        value::NgsiLdValue,
    },
    entity::{Entity, EntityPatch},
};
use andromeda_ngsi_ld_number::json_value::json_pointer::{JsonPointer, JsonPointerSegment};
use andromeda_ngsi_ld_representation::{
    attribute_type_validation::{AttributeNodeFault, AttributeTypeError, validate_attribute_node},
    compact_key_admission::{CompactKeyError, screen_keys_in_nested_value},
    embedded_context_admission::is_native_json_member,
    family_member_admission::MemberAdmission,
    proof_seal_admission::{ProofSealMember, proof_seal_admission},
};
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use serde_json::Value;
use thiserror::Error;

/// A compact payload one of the two rules refuses.
#[derive(Debug, Error)]
pub(crate) enum CompactRefusal {
    /// An attribute node is not one clauses 5.2.4 and 4.5.2.2 admit.
    #[error(transparent)]
    AttributeType(#[from] AttributeTypeError),
    /// A key the document cannot be written with: one JSON-LD expansion would
    /// drop or rewrite (clause 4.6.4).
    #[error(transparent)]
    Key(#[from] CompactKeyError),
}

/// Screen a decoded Entity under both rules.
///
/// # Errors
///
/// Returns the first attribute node clauses 5.2.4 and 4.5.2.2 refuse, else
/// the first key clause 4.6.4 refuses.
pub(crate) fn screen_compact_entity(entity: &Entity) -> Result<(), CompactRefusal> {
    screen_compact_members(entity.attribute_members())
}

/// Screen a decoded entity Fragment under both rules.
///
/// # Errors
///
/// As [`screen_compact_entity`].
pub(crate) fn screen_compact_patch(patch: &EntityPatch) -> Result<(), CompactRefusal> {
    let geo_properties = [
        (compact_properties::LOCATION, &patch.location),
        (compact_properties::OBSERVATION_SPACE, &patch.observation_space),
        (compact_properties::OPERATION_SPACE, &patch.operation_space),
    ];
    screen_compact_members(
        patch.attrs.iter().map(|(name, value)| (name.as_str(), value)).chain(
            geo_properties
                .into_iter()
                .filter_map(|(name, member)| member.as_ref().map(|member| (name, member))),
        ),
    )
}

/// Screen the attribute members of a decoded payload under both rules.
fn screen_compact_members<'a>(members: impl Iterator<Item = (&'a str, &'a AttributeValue)>) -> Result<(), CompactRefusal> {
    let mut members: Vec<_> = members.collect();
    // The rules walk a JSON object, whose members come in name order.
    members.sort_unstable_by_key(|(name, _)| *name);
    let members = members;
    members.iter().try_for_each(|(name, member)| screen_attribute_types(name, member))?;
    members
        .iter()
        .filter(|(name, _)| !is_native_json_member(name))
        .try_for_each(|(name, member)| screen_keys_in_member(member, &[segment(name)]))?;
    Ok(())
}

/// One member name as a pointer segment.
fn segment(name: &str) -> JsonPointerSegment {
    JsonPointerSegment::Member(Box::from(name))
}

/// A pointer the representation crate's walk reported from the node it was
/// handed, whose own name heads it, located instead from `at`.
fn rerooted(at: &[JsonPointerSegment], found: &JsonPointer) -> JsonPointer {
    let below = found.segments().split_first().map_or(&[][..], |(_, below)| below);
    JsonPointer::from_segments(at.iter().chain(below).cloned().collect())
}

/// `prefix` followed by one more segment.
fn extended(prefix: &[JsonPointerSegment], next: JsonPointerSegment) -> Vec<JsonPointerSegment> {
    prefix.iter().cloned().chain([next]).collect()
}

/// The clause 5.2.4 and 4.5.2.2 rule over one top-level attribute member.
fn screen_attribute_types(name: &str, member: &AttributeValue) -> Result<(), AttributeTypeError> {
    match member {
        AttributeValue::Raw(raw) => validate_attribute_node(name, raw),
        AttributeValue::Single(instance) => screen_instance(name, instance, &[segment(name)]),
        AttributeValue::Multiple(instances) => instances
            .iter()
            .enumerate()
            .try_for_each(|(index, instance)| screen_instance(name, instance, &[segment(name), JsonPointerSegment::Index(index)])),
    }
}

/// The clause 4.5.2.2 seal rule over one typed instance of the attribute
/// `name`, standing at `at`, and both rules over everything nested below it.
fn screen_instance(name: &str, instance: &Attribute, at: &[JsonPointerSegment]) -> Result<(), AttributeTypeError> {
    let sub_attributes = &instance.common().sub_attributes;
    let family = instance.attribute_type();
    let forbidden = sub_attributes
        .keys()
        .map(AttributeName::as_str)
        .filter(|member| proof_seal_admission(name, family, member) == MemberAdmission::Forbidden)
        .filter_map(|member| ProofSealMember::of(member).map(|seal| (member, seal)))
        .min_by(|(left, _), (right, _)| left.cmp(right));
    if let Some((_, member)) = forbidden {
        return Err(AttributeTypeError {
            attribute: JsonPointer::from_segments(at.to_vec()),
            fault: AttributeNodeFault::ProofSealOutsideNgsildproof(member),
        });
    }
    // The family's own members are its envelope, not attributes nested in it.
    let reserved = family.reserved_fields();
    let mut nested: Vec<_> = sub_attributes
        .iter()
        .filter(|(member, _)| ProofSealMember::of(member.as_str()).is_none() && !reserved.contains(&member.as_str()))
        .collect();
    nested.sort_unstable_by_key(|(name, _)| *name);
    nested
        .into_iter()
        .try_for_each(|(member, value)| screen_member(member.as_str(), value, &extended(at, segment(member.as_str()))))
}

/// Both rules over one nested member, standing at `at`.
fn screen_member(name: &str, member: &AttributeValue, at: &[JsonPointerSegment]) -> Result<(), AttributeTypeError> {
    match member {
        AttributeValue::Single(instance) => screen_instance(name, instance, at),
        AttributeValue::Multiple(instances) => instances
            .iter()
            .enumerate()
            .try_for_each(|(index, instance)| screen_instance(name, instance, &extended(at, JsonPointerSegment::Index(index)))),
        AttributeValue::Raw(node) => screen_written_node(name, node, at),
    }
}

/// Both rules over one nested node carried as written.
///
/// A sub-attribute is an attribute (clause 4.5.2.2), so the representation
/// crate's walk reads it exactly as it reads a top-level one, and its finding
/// is located from the top of the entity.
fn screen_written_node(name: &str, node: &Value, at: &[JsonPointerSegment]) -> Result<(), AttributeTypeError> {
    validate_attribute_node(name, node).map_err(|error| AttributeTypeError {
        attribute: rerooted(at, &error.attribute),
        fault: error.fault,
    })
}

/// The key rules over one attribute member standing at `at`.
fn screen_keys_in_member(member: &AttributeValue, at: &[JsonPointerSegment]) -> Result<(), CompactKeyError> {
    match member {
        AttributeValue::Raw(raw) => screen_keys_in_written(raw, at),
        AttributeValue::Single(instance) => screen_keys_in_instance(instance, at),
        AttributeValue::Multiple(instances) => instances
            .iter()
            .enumerate()
            .try_for_each(|(index, instance)| screen_keys_in_instance(instance, &extended(at, JsonPointerSegment::Index(index)))),
    }
}

/// The key rules over one typed instance standing at `at`: its client-written
/// JSON values and its sub-attributes, in member order. Its own keys are the
/// typed members of its family and attribute names, none of which the rules
/// refuse; a `@context` among them is the user `@context` admission's to
/// refuse, before this is reached.
fn screen_keys_in_instance(instance: &Attribute, at: &[JsonPointerSegment]) -> Result<(), CompactKeyError> {
    let sub_attributes = &instance.common().sub_attributes;
    let mut slots: Vec<(&str, WrittenSlot<'_>)> = written_values(instance);
    slots.extend(
        sub_attributes
            .iter()
            .filter(|(name, _)| !is_native_json_member(name.as_str()))
            .map(|(name, value)| (name.as_str(), WrittenSlot::SubAttribute(value))),
    );
    slots.sort_unstable_by_key(|(name, _)| *name);
    slots.into_iter().try_for_each(|(name, slot)| {
        let at = extended(at, segment(name));
        match slot {
            WrittenSlot::Value(value) => screen_keys_in_value(value, &at),
            WrittenSlot::Values(values) => values
                .iter()
                .enumerate()
                .try_for_each(|(index, value)| screen_keys_in_value(value, &extended(&at, JsonPointerSegment::Index(index)))),
            WrittenSlot::Linked(OneOrMany::One(entity)) => screen_keys_in_entity(entity, &at),
            WrittenSlot::Linked(OneOrMany::Many(entities)) => entities
                .iter()
                .enumerate()
                .try_for_each(|(index, entity)| screen_keys_in_entity(entity, &extended(&at, JsonPointerSegment::Index(index)))),
            WrittenSlot::SubAttribute(member) => screen_keys_in_member(member, &at),
        }
    })
}

/// A member of a typed instance that can carry a key the rules refuse.
enum WrittenSlot<'a> {
    /// One client-written value.
    Value(&'a NgsiLdValue),
    /// An ordered list of client-written values.
    Values(&'a [NgsiLdValue]),
    /// Inline Linked Entities a client wrote.
    Linked(&'a InlineEntity),
    /// A sub-attribute.
    SubAttribute(&'a AttributeValue),
}

/// The members of `instance`'s own family that hold client-written JSON.
fn written_values(instance: &Attribute) -> Vec<(&'static str, WrittenSlot<'_>)> {
    match instance {
        Attribute::Property(property) => [Some((compact_properties::VALUE, WrittenSlot::Value(&property.value)))]
            .into_iter()
            .chain([property
                .previous_value
                .as_ref()
                .map(|previous| (compact_properties::PREVIOUS_VALUE, WrittenSlot::Value(previous)))])
            .flatten()
            .collect(),
        Attribute::ListProperty(list_property) => [Some((compact_properties::VALUE_LIST, WrittenSlot::Values(&list_property.value_list)))]
            .into_iter()
            .chain([list_property
                .previous_value_list
                .as_deref()
                .map(|previous| (compact_properties::PREVIOUS_VALUE_LIST, WrittenSlot::Values(previous)))])
            .flatten()
            .collect(),
        Attribute::Relationship(relationship) => relationship
            .entity
            .iter()
            .map(|entities| (compact_properties::ENTITY, WrittenSlot::Linked(entities)))
            .collect(),
        Attribute::ListRelationship(list_relationship) => list_relationship
            .entity_list
            .iter()
            .map(|entities| (compact_properties::ENTITY_LIST, WrittenSlot::Linked(entities)))
            .collect(),
        Attribute::GeoProperty(_) | Attribute::VocabProperty(_) | Attribute::LanguageProperty(_) | Attribute::JsonProperty(_) => Vec::new(),
    }
}

/// The key rules over one inline Linked Entity standing at `at`: its attribute
/// members, in member order.
fn screen_keys_in_entity(entity: &Entity, at: &[JsonPointerSegment]) -> Result<(), CompactKeyError> {
    let mut members: Vec<_> = entity.attribute_members().filter(|(name, _)| !is_native_json_member(name)).collect();
    members.sort_unstable_by_key(|(name, _)| *name);
    members
        .into_iter()
        .try_for_each(|(name, member)| screen_keys_in_member(member, &extended(at, segment(name))))
}

/// The key rules over one client-written value; a typed geometry holds
/// positions only and carries nothing to refuse.
fn screen_keys_in_value(value: &NgsiLdValue, at: &[JsonPointerSegment]) -> Result<(), CompactKeyError> {
    match value {
        NgsiLdValue::Json(json) => screen_keys_in_written(json, at),
        NgsiLdValue::Geometry(_) => Ok(()),
    }
}

/// The key rules over one value carried as written, standing at `at` below
/// the top level, read by the representation crate's own walk and located
/// from the top of the entity.
fn screen_keys_in_written(value: &Value, at: &[JsonPointerSegment]) -> Result<(), CompactKeyError> {
    screen_keys_in_nested_value(value).map_err(|error| error.relocated(|path| located(at, path)))
}

/// A pointer the representation crate's walk reported from the value it was
/// handed, located instead from `at`.
fn located(at: &[JsonPointerSegment], found: &JsonPointer) -> JsonPointer {
    JsonPointer::from_segments(at.iter().chain(found.segments()).cloned().collect())
}

#[cfg(test)]
mod tests {
    use crate::compact_screening::{CompactRefusal, screen_compact_entity, screen_compact_patch};
    use andromeda_ngsi_ld_model::entity::{Entity, EntityPatch};
    use andromeda_ngsi_ld_representation::{attribute_type_validation::validate_fragment_attribute_types, compact_key_admission::screen_keys_in_fragment};
    use serde_json::{Value, json};

    /// What the two rules decide over the compact JSON of a decoded payload,
    /// as they were run before the payload stayed typed.
    fn over_rendered_json(rendered: &Value) -> Result<(), String> {
        validate_fragment_attribute_types(rendered).map_err(|error| error.to_string())?;
        screen_keys_in_fragment(rendered).map_err(|error| error.to_string())
    }

    /// The finding of the typed screening, as text.
    fn typed(outcome: Result<(), CompactRefusal>) -> Result<(), String> {
        outcome.map_err(|error| error.to_string())
    }

    /// Entities each breaking, or narrowly keeping, one of the rules at a
    /// different depth and in a different shape.
    fn corpus() -> Vec<Value> {
        let entity = |members: Value| {
            let mut object = json!({"id": "urn:ngsi-ld:Building:1", "type": "Building"});
            if let (Some(target), Value::Object(members)) = (object.as_object_mut(), members) {
                target.extend(members);
            }
            object
        };
        vec![
            entity(json!({
                "name": {"type": "Property", "value": "x", "unitCode": "CEL", "observedAt": "2025-01-01T00:00:00Z"},
                "owner": {"type": "Relationship", "object": "urn:ngsi-ld:P:1"},
                "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}},
                "street": {"type": "LanguageProperty", "languageMap": {"en": "x"}},
                "config": {"type": "JsonProperty", "json": {"@context": "data, not a context"}},
                "list": {"type": "ListProperty", "valueList": [1, {"a": 2}]},
                "stops": {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:S:1"]},
                "kind": {"type": "VocabProperty", "vocab": "x"}
            })),
            entity(json!({"belongsTo": {"type": "Relationship", "object": "urn:ngsi-ld:Site:1", "entityIdSealed": "urn:ngsi-ld:Building:1"}})),
            entity(json!({"belongsTo": {"object": "urn:ngsi-ld:Site:1", "entityTypeSealed": "Building"}})),
            entity(
                json!({"ngsildproof": {"type": "Property", "value": {"cryptosuite": "eddsa-jcs-2022"}, "entityIdSealed": "urn:x:1", "entityTypeSealed": "Building"}}),
            ),
            entity(json!({"ngsildproof": {"type": "Relationship", "object": "urn:x:1", "entityIdSealed": "urn:x:1"}})),
            entity(json!({"temperature": {"type": "NotAnAttributeType", "value": 21.5}})),
            entity(json!({"speed": {"type": "Property", "value": 1, "accuracy": {"type": "Property", "value": 0.5, "entityIdSealed": "x"}}})),
            entity(json!({"speed": {"type": "Property", "value": 1, "accuracy": {"value": 0.5, "entityTypeSealed": "x"}}})),
            entity(json!({"speed": {"type": "Property", "value": 1, "readings": [{"value": 1}, 7, {"value": 2, "entityIdSealed": "x"}]}})),
            entity(
                json!({"speed": {"type": "Property", "value": 1, "odd": {"type": "Bogus", "deeper": {"type": "Property", "value": 1, "entityIdSealed": "x"}}}}),
            ),
            entity(json!({"speed": [{"type": "Property", "value": 1}, {"type": "Property", "value": 2, "datasetId": "urn:ds:1", "entityTypeSealed": "x"}]})),
            entity(json!({"speed": {"type": "Property", "value": 1, "entityTypeSealed": "x", "entityIdSealed": "y"}})),
            entity(json!({"speed": {"type": "Property", "value": 1, "name": {"type": "Property", "value": 1, "entityIdSealed": "x"}}})),
            entity(json!({"speed": {"type": "Property", "value": 1, "accuracy": {"type": "Propertyy", "value": 0.5}}})),
            entity(json!({"speed": {"type": "Property", "value": 1, "accuracy": {"type": 7, "value": 0.5}}})),
            entity(json!({"speed": {"type": "Property", "value": 1, "spot": {"type": "Point"}}})),
            entity(json!({"speed": {"type": "Property", "value": 1, "both": {"object": "urn:x:1", "vocab": "x"}}})),
            entity(json!({"speed": {"type": "Property", "value": 1, "readings": [{"value": 1, "datasetId": "urn:ds:1"}, 7]}})),
            entity(json!({"speed": {"type": "Property", "value": 1, "accuracy": {"type": "Property", "value": {"type": "Feature"}}}})),
            entity(json!({"p": {"type": "Property", "value": {"opening hours": "9-5", "id": "abc", "type": "Postal Address"}}})),
            entity(json!({"p": {"type": "Property", "value": {"@foo": "x", "name": "y"}}})),
            entity(json!({"p": {"type": "Property", "value": 1, "sub": {"value": {"a": [{"@Bar": 1}]}}}})),
            entity(json!({"p": {"type": "Property", "value": {"": "x"}}})),
            entity(json!({"p": {"type": "ListProperty", "valueList": [1, {"": 2}]}})),
            entity(json!({"p": {"type": "Property", "value": {"type": ""}}})),
            entity(json!({"p": {"type": "Property", "value": {"@type": ["A", ""]}}, "q": {"type": "Property", "value": {"@foo": 1}}})),
            entity(json!({"label": {"type": "LanguageProperty", "languageMap": {"en": "x", "@none": "y"}}})),
        ]
    }

    /// Both rules reach the same verdict and name the same location on the
    /// typed entity as on its compact JSON, for every entity of the corpus.
    #[test]
    fn the_typed_screening_finds_what_the_rules_find_on_the_compact_json() {
        for written in corpus() {
            let entity: Entity = serde_json::from_value(written.clone()).expect("the corpus entity decodes"); // clone: the expectation names the payload
            let rendered = serde_json::to_value(&entity).expect("the entity renders");
            pretty_assertions::assert_eq!(typed(screen_compact_entity(&entity)), over_rendered_json(&rendered), "{written}");
        }
    }

    /// An entity Fragment is screened over the same members, its three
    /// geo-properties included.
    #[test]
    fn a_fragment_is_screened_as_the_entity_it_patches() {
        for written in corpus() {
            let patch: EntityPatch = serde_json::from_value(written.clone()).expect("the corpus fragment decodes"); // clone: the expectation names the payload
            let rendered = serde_json::to_value(&patch).expect("the fragment renders");
            pretty_assertions::assert_eq!(typed(screen_compact_patch(&patch)), over_rendered_json(&rendered), "{written}");
        }
    }

    /// A sub-attribute is an attribute (clause 4.5.2.2): one the decode carried
    /// as written because its `type` names no attribute type is refused where
    /// it stands, in an Entity and in a Fragment alike.
    #[test]
    fn a_misspelt_type_on_a_written_sub_attribute_is_refused_where_it_stands() {
        let written = json!({
            "id": "urn:ngsi-ld:Building:1",
            "type": "Building",
            "speed": {"type": "Property", "value": 1, "accuracy": {"type": "Propertyy", "value": 0.5}}
        });
        let entity: Entity = serde_json::from_value(written.clone()).expect("the entity decodes"); // clone: the fragment is decoded from the same payload
        let patch: EntityPatch = serde_json::from_value(written).expect("the fragment decodes");
        for refusal in [screen_compact_entity(&entity), screen_compact_patch(&patch)] {
            let refusal = typed(refusal).expect_err("a misspelt sub-attribute type is refused");
            assert!(refusal.contains("/speed/accuracy"), "{refusal}");
            assert!(refusal.contains("not an NGSI-LD attribute type"), "{refusal}");
        }
    }

    /// The corpus is not vacuous: it holds payloads each rule refuses.
    #[test]
    fn the_corpus_holds_refusals_of_both_rules() {
        let findings: Vec<String> = corpus()
            .into_iter()
            .filter_map(|written| serde_json::from_value::<Entity>(written).ok())
            .filter_map(|entity| screen_compact_entity(&entity).err())
            .map(|error| error.to_string())
            .collect();
        assert!(findings.iter().any(|finding| finding.contains("ngsildproof")), "{findings:?}");
        assert!(findings.iter().any(|finding| finding.contains("not an NGSI-LD attribute type")), "{findings:?}");
        assert!(findings.iter().any(|finding| finding.contains("form of a JSON-LD keyword")), "{findings:?}");
        assert!(findings.iter().any(|finding| finding.contains("is empty")), "{findings:?}");
    }
}
