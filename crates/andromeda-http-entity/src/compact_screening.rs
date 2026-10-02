//! The two clause 5.5.4 rules decidable only on the compact payload, read off
//! the typed compact entity.
//!
//! [`validate_fragment_attribute_types`] and
//! [`reject_embedded_contexts_in_fragment`] state the rules over the compact
//! JSON of an entity: clause 5.2.4's eight attribute types together with
//! clause 4.5.2.2's `ngsildproof` seals, and clause 5.5.7's refusal of a
//! `@context` below the top level. The extractor decodes the request straight
//! into the typed [`Entity`] or [`EntityPatch`], so the rules are applied here
//! to what that decode holds, member by member, rather than to a JSON copy of
//! the whole entity rendered back for them:
//!
//! - a member the decode carries verbatim, and every client-written JSON value
//!   inside a typed instance, is read by the representation crate's own walk,
//!   its location reported from the top of the entity;
//! - a typed instance declares one of the eight types by construction, holds
//!   any seal it was written with as a sub-attribute, and can carry an
//!   embedded `@context` only inside a JSON value or a sub-attribute, so only
//!   those are read;
//! - a typed geometry is RFC 7946 positions and nothing else, so neither rule
//!   has anything to read in it, which is what keeps a large geometry out of
//!   the screening.
//!
//! Each rule reports the first fault in the member order of the compact JSON
//! it is stated over, and the attribute-type rule is decided over every member
//! before the context rule, exactly as the two run over a rendered entity.
//!
//! [`validate_fragment_attribute_types`]: andromeda_ngsi_ld_representation::attribute_type_validation::validate_fragment_attribute_types
//! [`reject_embedded_contexts_in_fragment`]: andromeda_ngsi_ld_representation::embedded_context_admission::reject_embedded_contexts_in_fragment

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
    attribute_type_validation::{AttributeTypeError, validate_attribute_node},
    embedded_context_admission::{EmbeddedContextError, is_native_json_member, reject_embedded_contexts_in_attribute},
    family_member_admission::MemberAdmission,
    proof_seal_admission::{ProofSealMember, proof_seal_admission},
};
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use serde_json::Value;
use thiserror::Error;

/// The member a JSON-LD document declares its context in.
const CONTEXT_MEMBER: &str = "@context";

/// A compact payload one of the two rules refuses.
#[derive(Debug, Error)]
pub(crate) enum CompactRefusal {
    /// An attribute node is not one clauses 5.2.4 and 4.5.2.2 admit.
    #[error(transparent)]
    AttributeType(#[from] AttributeTypeError),
    /// A `@context` stands below the top level of the document (clause 5.5.7).
    #[error(transparent)]
    EmbeddedContext(#[from] EmbeddedContextError),
}

/// Screen a decoded Entity under both rules.
///
/// # Errors
///
/// Returns the first attribute node clauses 5.2.4 and 4.5.2.2 refuse, else
/// the first embedded `@context` clause 5.5.7 refuses.
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
        .try_for_each(|(name, member)| reject_contexts_in_member(member, &[segment(name)]))?;
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
        AttributeValue::Single(instance) => seals_of_instance(name, instance, &[segment(name)]),
        AttributeValue::Multiple(instances) => instances
            .iter()
            .enumerate()
            .try_for_each(|(index, instance)| seals_of_instance(name, instance, &[segment(name), JsonPointerSegment::Index(index)])),
    }
}

/// The clause 4.5.2.2 seal rule over one typed instance of the attribute
/// `name`, standing at `at`, and over everything nested below it.
fn seals_of_instance(name: &str, instance: &Attribute, at: &[JsonPointerSegment]) -> Result<(), AttributeTypeError> {
    let sub_attributes = &instance.common().sub_attributes;
    let family = instance.attribute_type();
    let forbidden = sub_attributes
        .keys()
        .map(AttributeName::as_str)
        .filter(|member| proof_seal_admission(name, family, member) == MemberAdmission::Forbidden)
        .filter_map(|member| ProofSealMember::of(member).map(|seal| (member, seal)))
        .min_by(|(left, _), (right, _)| left.cmp(right));
    if let Some((_, member)) = forbidden {
        return Err(AttributeTypeError::ProofSealOutsideNgsildproof {
            attribute: JsonPointer::from_segments(at.to_vec()),
            member,
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
        .try_for_each(|(member, value)| seals_of_member(member.as_str(), value, &extended(at, segment(member.as_str()))))
}

/// The clause 4.5.2.2 seal rule over one nested member, standing at `at`.
fn seals_of_member(name: &str, member: &AttributeValue, at: &[JsonPointerSegment]) -> Result<(), AttributeTypeError> {
    match member {
        AttributeValue::Single(instance) => seals_of_instance(name, instance, at),
        AttributeValue::Multiple(instances) => instances
            .iter()
            .enumerate()
            .try_for_each(|(index, instance)| seals_of_instance(name, instance, &extended(at, JsonPointerSegment::Index(index)))),
        AttributeValue::Raw(Value::Array(items)) => items
            .iter()
            .enumerate()
            .try_for_each(|(index, item)| seals_of_written_node(name, item, &extended(at, JsonPointerSegment::Index(index)))),
        AttributeValue::Raw(node) => seals_of_written_node(name, node, at),
    }
}

/// The clause 4.5.2.2 seal rule over one nested node carried as written.
///
/// The representation crate's walk reads a node as a top-level attribute, and
/// so also checks the `type` it declares; below the top level only the seals
/// are the rule, so the type finding of the node itself is set aside, and a
/// seal finding is located from the top of the entity.
fn seals_of_written_node(name: &str, node: &Value, at: &[JsonPointerSegment]) -> Result<(), AttributeTypeError> {
    if !node.is_object() {
        return Ok(());
    }
    match validate_attribute_node(name, node) {
        Ok(()) | Err(AttributeTypeError::NotAnAttributeType { .. }) => Ok(()),
        Err(AttributeTypeError::ProofSealOutsideNgsildproof { attribute, member }) => Err(AttributeTypeError::ProofSealOutsideNgsildproof {
            attribute: rerooted(at, &attribute),
            member,
        }),
    }
}

/// The clause 5.5.7 rule over one attribute member standing at `at`.
fn reject_contexts_in_member(member: &AttributeValue, at: &[JsonPointerSegment]) -> Result<(), EmbeddedContextError> {
    match member {
        AttributeValue::Raw(raw) => reject_contexts_in_written(raw, at),
        AttributeValue::Single(instance) => reject_contexts_in_instance(instance, at),
        AttributeValue::Multiple(instances) => instances
            .iter()
            .enumerate()
            .try_for_each(|(index, instance)| reject_contexts_in_instance(instance, &extended(at, JsonPointerSegment::Index(index)))),
    }
}

/// The clause 5.5.7 rule over one typed instance standing at `at`: its
/// client-written JSON values and its sub-attributes, in member order.
fn reject_contexts_in_instance(instance: &Attribute, at: &[JsonPointerSegment]) -> Result<(), EmbeddedContextError> {
    let sub_attributes = &instance.common().sub_attributes;
    if sub_attributes.contains_key(CONTEXT_MEMBER) {
        return Err(EmbeddedContextError::BelowTopLevel {
            path: JsonPointer::from_segments(at.to_vec()),
        });
    }
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
            WrittenSlot::Value(value) => reject_contexts_in_value(value, &at),
            WrittenSlot::Values(values) => values
                .iter()
                .enumerate()
                .try_for_each(|(index, value)| reject_contexts_in_value(value, &extended(&at, JsonPointerSegment::Index(index)))),
            WrittenSlot::Linked(OneOrMany::One(entity)) => reject_contexts_in_entity(entity, &at),
            WrittenSlot::Linked(OneOrMany::Many(entities)) => entities
                .iter()
                .enumerate()
                .try_for_each(|(index, entity)| reject_contexts_in_entity(entity, &extended(&at, JsonPointerSegment::Index(index)))),
            WrittenSlot::SubAttribute(member) => reject_contexts_in_member(member, &at),
        }
    })
}

/// A member of a typed instance that can carry an embedded `@context`.
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

/// The clause 5.5.7 rule over one inline Linked Entity standing at `at`: its
/// attribute members, in member order.
fn reject_contexts_in_entity(entity: &Entity, at: &[JsonPointerSegment]) -> Result<(), EmbeddedContextError> {
    let mut members: Vec<_> = entity.attribute_members().filter(|(name, _)| !is_native_json_member(name)).collect();
    members.sort_unstable_by_key(|(name, _)| *name);
    members
        .into_iter()
        .try_for_each(|(name, member)| reject_contexts_in_member(member, &extended(at, segment(name))))
}

/// The clause 5.5.7 rule over one client-written value; a typed geometry
/// holds positions only and carries nothing to refuse.
fn reject_contexts_in_value(value: &NgsiLdValue, at: &[JsonPointerSegment]) -> Result<(), EmbeddedContextError> {
    match value {
        NgsiLdValue::Json(json) => reject_contexts_in_written(json, at),
        NgsiLdValue::Geometry(_) => Ok(()),
    }
}

/// The clause 5.5.7 rule over one value carried as written, standing at `at`
/// below the top level: an object carrying a `@context` is refused, and every
/// member it holds that is no native JSON literal is walked.
fn reject_contexts_in_written(value: &Value, at: &[JsonPointerSegment]) -> Result<(), EmbeddedContextError> {
    match value {
        Value::Object(members) => {
            if members.contains_key(CONTEXT_MEMBER) {
                return Err(EmbeddedContextError::BelowTopLevel {
                    path: JsonPointer::from_segments(at.to_vec()),
                });
            }
            // The representation crate walks the members of a node it reads
            // as an attribute exactly as it walks those of any nested object;
            // its finding is located from the top of the entity.
            reject_embedded_contexts_in_attribute("", value)
                .map_err(|EmbeddedContextError::BelowTopLevel { path }| EmbeddedContextError::BelowTopLevel { path: rerooted(at, &path) })
        }
        Value::Array(items) => items
            .iter()
            .enumerate()
            .try_for_each(|(index, item)| reject_contexts_in_written(item, &extended(at, JsonPointerSegment::Index(index)))),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use crate::compact_screening::{CompactRefusal, screen_compact_entity, screen_compact_patch};
    use andromeda_ngsi_ld_model::entity::{Entity, EntityPatch};
    use andromeda_ngsi_ld_representation::{
        attribute_type_validation::validate_fragment_attribute_types,
        embedded_context_admission::reject_embedded_contexts_in_fragment,
    };
    use serde_json::{Value, json};

    /// What the two rules decide over the compact JSON of a decoded payload,
    /// as they were run before the payload stayed typed.
    fn over_rendered_json(rendered: &Value) -> Result<(), String> {
        validate_fragment_attribute_types(rendered).map_err(|error| error.to_string())?;
        reject_embedded_contexts_in_fragment(rendered).map_err(|error| error.to_string())
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
            entity(json!({"p": {"type": "Property", "value": {"a": {"@context": {}}}}})),
            entity(json!({"p": {"value": 1, "@context": "https://example.org/ctx.jsonld"}})),
            entity(json!({"p": [{"value": 1}, {"value": 2, "nested": {"@context": {}}}]})),
            entity(json!({"p": {"type": "Property", "value": 1, "json": {"x": {"@context": {}}}}})),
            entity(json!({"p": {"type": "ListProperty", "valueList": [1, {"a": [{"@context": {}}]}]}})),
            entity(json!({"p": {"type": "Property", "value": 1, "sub": {"type": "Property", "value": {"@context": {}}}}})),
            entity(json!({"p": [{"type": "Property", "value": 1}, {"type": "Property", "value": {"@context": {}}, "datasetId": "urn:ds:2"}]})),
            entity(json!({"a": {"type": "Propertyy", "value": 1}, "b": {"type": "Property", "value": {"@context": {}}}})),
            entity(json!({"location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0], "@context": {}}}})),
            entity(json!({"json": {"type": "Property", "value": {"@context": {}}}})),
            entity(json!({"p": {"type": "Property", "value": 1, "sub": [{"value": 1}, {"value": {"deep": [{"@context": {}}]}}]}})),
            entity(json!({"z": {"type": "Property", "value": {"@context": {}}}, "a": {"type": "Property", "value": {"x": {"@context": {}}}}})),
            entity(
                json!({"r": {"type": "Relationship", "object": "urn:x:1", "entity": {"id": "urn:x:1", "type": "T", "q": {"type": "Property", "value": {"@context": {}}}}}}),
            ),
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
        assert!(findings.iter().any(|finding| finding.contains("@context is embedded")), "{findings:?}");
    }
}
