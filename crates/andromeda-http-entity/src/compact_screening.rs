//! The clause 4.6.4 key rule, read off the typed compact entity.
//!
//! [`screen_keys_in_fragment`] states the rule over the compact JSON of an
//! entity: the keys JSON-LD expansion would drop or rewrite past recovery.
//! Clause 5.5.7's `@context` below the top level is not among them: it is part
//! of the request's user `@context`, refused by the user `@context` admission
//! (`andromeda_jsonld::user_context_admission`) before this screening runs. The
//! attribute types of clause 5.2.4 and the seals of clause 4.5.2.2 are not
//! among them either: the decode of the request refuses a node either rule
//! refuses, so a decoded payload holds none. The extractor decodes the request
//! straight into the typed [`Entity`] or [`EntityPatch`], so the rule is
//! applied here to what that decode holds, member by member, rather than to a
//! JSON copy of the whole entity rendered back for it:
//!
//! - every client-written JSON value inside a typed instance is read by the
//!   representation crate's own walk, its location reported from the top of
//!   the entity;
//! - a typed instance is keyed by its family's members and by attribute names,
//!   so it can carry a refused key only inside a JSON value or a sub-attribute,
//!   and only those are read;
//! - a typed geometry is RFC 7946 positions and nothing else, so the rule has
//!   nothing to read in it, which is what keeps a large geometry out of the
//!   screening.
//!
//! The rule reports the first fault in the member order of the compact JSON it
//! is stated over.
//!
//! [`screen_keys_in_fragment`]: andromeda_ngsi_ld_representation::compact_key_admission::screen_keys_in_fragment

use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_expansion::written_member::{WrittenMember, WrittenMemberRef};
use andromeda_ngsi_ld_model::{
    attribute::{
        inline_entity::InlineEntity,
        instance::{Attribute, AttributeValue},
        merge_instance::MergeInstance,
        merge_member::MergeMember,
        merge_value::MergeValue,
        one_or_many::OneOrMany,
        patched::Patched,
        value::NgsiLdValue,
    },
    entity::{Entity, EntityPatch},
};
use andromeda_ngsi_ld_number::json_value::json_pointer::{JsonPointer, JsonPointerSegment};
use andromeda_ngsi_ld_representation::{
    compact_key_admission::{CompactKeyError, screen_keys_in_nested_value},
    embedded_context_admission::is_native_json_member,
};
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;

/// Screen the attribute members of a decoded Entity.
///
/// # Errors
///
/// Returns the first key clause 4.6.4 refuses.
pub(crate) fn screen_compact_entity<M: WrittenMember>(entity: &Entity<M>) -> Result<(), CompactKeyError> {
    screen_compact_members(entity.attribute_members())
}

/// Screen the attribute members of a decoded entity Fragment.
///
/// # Errors
///
/// As [`screen_compact_entity`].
pub(crate) fn screen_compact_patch<M: WrittenMember>(patch: &EntityPatch<M>) -> Result<(), CompactKeyError> {
    screen_compact_members(patch.attribute_members())
}

/// Screen the one attribute member a bare Attribute Fragment of clauses 5.6.4
/// and 5.6.19 writes to the attribute `name`.
///
/// # Errors
///
/// As [`screen_compact_entity`], located from the attribute's name.
pub(crate) fn screen_compact_attribute<M: WrittenMember>(name: &str, member: &M) -> Result<(), CompactKeyError> {
    if is_native_json_member(name) {
        return Ok(());
    }
    screen_keys_in_written_member(member, &[segment(name)])
}

/// Screen the attribute members of a decoded payload.
fn screen_compact_members<'a, M: WrittenMember + 'a>(members: impl Iterator<Item = (&'a str, &'a M)>) -> Result<(), CompactKeyError> {
    let mut members: Vec<_> = members.filter(|(name, _)| !is_native_json_member(name)).collect();
    // The rule walks a JSON object, whose members come in name order.
    members.sort_unstable_by_key(|(name, _)| *name);
    members
        .into_iter()
        .try_for_each(|(name, member)| screen_keys_in_written_member(member, &[segment(name)]))
}

/// The key rule over one member of an entity or of a merge fragment, standing
/// at `at`.
fn screen_keys_in_written_member<M: WrittenMember>(member: &M, at: &[JsonPointerSegment]) -> Result<(), CompactKeyError> {
    match member.written() {
        WrittenMemberRef::Attribute(member) => screen_keys_in_member(member, at),
        WrittenMemberRef::Merge(member) => screen_keys_in_merge_member(member, at),
    }
}

/// One member name as a pointer segment.
fn segment(name: &str) -> JsonPointerSegment {
    JsonPointerSegment::Member(Box::from(name))
}

/// `prefix` followed by one more segment.
fn extended(prefix: &[JsonPointerSegment], next: JsonPointerSegment) -> Vec<JsonPointerSegment> {
    prefix.iter().cloned().chain([next]).collect()
}

/// The key rule over one attribute member standing at `at`; the NGSI-LD Null
/// carries no key.
fn screen_keys_in_member(member: &AttributeValue, at: &[JsonPointerSegment]) -> Result<(), CompactKeyError> {
    match member {
        AttributeValue::Single(instance) => screen_keys_in_instance(instance, at),
        AttributeValue::Multiple(instances) => instances
            .iter()
            .enumerate()
            .try_for_each(|(index, instance)| screen_keys_in_instance(instance, &extended(at, JsonPointerSegment::Index(index)))),
        AttributeValue::NgsiLdNull => Ok(()),
    }
}

/// The key rule over one typed instance standing at `at`: its client-written
/// JSON values and its sub-attributes, in member order. Its own keys are the
/// typed members of its family and attribute names, none of which the rule
/// refuses; a `@context` among them is the user `@context` admission's to
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
            WrittenSlot::Literal(literal) => screen_keys_in_written(literal, &at),
            WrittenSlot::Linked(OneOrMany::One(entity)) => screen_keys_in_entity(entity, &at),
            WrittenSlot::Linked(OneOrMany::Many(entities)) => entities
                .iter()
                .enumerate()
                .try_for_each(|(index, entity)| screen_keys_in_entity(entity, &extended(&at, JsonPointerSegment::Index(index)))),
            WrittenSlot::SubAttribute(member) => screen_keys_in_member(member, &at),
        }
    })
}

/// A member of a typed instance that can carry a key the rule refuses.
enum WrittenSlot<'a> {
    /// One client-written value.
    Value(&'a NgsiLdValue),
    /// An ordered list of client-written values.
    Values(&'a [NgsiLdValue]),
    /// A client-written JSON value the family holds as written.
    Literal(&'a JsonLiteral),
    /// Inline Linked Entities a client wrote.
    Linked(&'a InlineEntity),
    /// A sub-attribute.
    SubAttribute(&'a AttributeValue),
}

/// The members of `instance`'s own family that hold client-written JSON.
fn written_values(instance: &Attribute) -> Vec<(&'static str, WrittenSlot<'_>)> {
    match instance {
        Attribute::Property(property) => [
            Some((compact_properties::VALUE, WrittenSlot::Value(&property.value))),
            property
                .previous_value
                .as_ref()
                .map(|previous| (compact_properties::PREVIOUS_VALUE, WrittenSlot::Value(previous))),
            property
                .entity_id_sealed
                .as_ref()
                .map(|seal| (compact_properties::ENTITY_ID_SEALED, WrittenSlot::Literal(seal))),
            property
                .entity_type_sealed
                .as_ref()
                .map(|seal| (compact_properties::ENTITY_TYPE_SEALED, WrittenSlot::Literal(seal))),
        ]
        .into_iter()
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

/// The key rule over one merge member standing at `at`; the NGSI-LD Null
/// carries no key.
fn screen_keys_in_merge_member(member: &MergeMember, at: &[JsonPointerSegment]) -> Result<(), CompactKeyError> {
    match member {
        MergeMember::Single(instance) => screen_keys_in_merge_instance(instance, at),
        MergeMember::Multiple(instances) => instances
            .iter()
            .enumerate()
            .try_for_each(|(index, instance)| screen_keys_in_merge_instance(instance, &extended(at, JsonPointerSegment::Index(index)))),
        MergeMember::NgsiLdNull => Ok(()),
    }
}

/// The key rule over one merge instance standing at `at`: the value it
/// writes, its seals and its sub-attributes, in member order.
fn screen_keys_in_merge_instance(instance: &MergeInstance, at: &[JsonPointerSegment]) -> Result<(), CompactKeyError> {
    let mut slots: Vec<(&str, MergeSlot<'_>)> = [
        match &instance.value {
            Some(Patched::Set(value)) => Some((value.family().compact_value_key(), MergeSlot::Value(value))),
            Some(Patched::Null) | None => None,
        },
        instance
            .entity_id_sealed
            .as_ref()
            .map(|seal| (compact_properties::ENTITY_ID_SEALED, MergeSlot::Literal(seal))),
        instance
            .entity_type_sealed
            .as_ref()
            .map(|seal| (compact_properties::ENTITY_TYPE_SEALED, MergeSlot::Literal(seal))),
    ]
    .into_iter()
    .flatten()
    .collect();
    slots.extend(
        instance
            .sub_attributes
            .iter()
            .filter(|(name, _)| !is_native_json_member(name.as_str()))
            .map(|(name, member)| (name.as_str(), MergeSlot::SubAttribute(member))),
    );
    slots.sort_unstable_by_key(|(name, _)| *name);
    slots.into_iter().try_for_each(|(name, slot)| {
        let at = extended(at, segment(name));
        match slot {
            MergeSlot::Value(value) => screen_keys_in_merge_value(value, &at),
            MergeSlot::Literal(literal) => screen_keys_in_written(literal, &at),
            MergeSlot::SubAttribute(member) => screen_keys_in_merge_member(member, &at),
        }
    })
}

/// A member of a merge instance that can carry a key the rule refuses.
enum MergeSlot<'a> {
    /// The value the instance writes.
    Value(&'a MergeValue),
    /// A client-written JSON value held as written.
    Literal(&'a JsonLiteral),
    /// A sub-attribute.
    SubAttribute(&'a MergeMember),
}

/// The key rule over the value a merge instance writes. A JSON literal of a
/// `JsonProperty` is data the processor never expands, a language map is
/// keyed by tags, and the identifiers of the relationship and vocabulary
/// families are strings, so only a Property's values carry keys.
fn screen_keys_in_merge_value(value: &MergeValue, at: &[JsonPointerSegment]) -> Result<(), CompactKeyError> {
    match value {
        MergeValue::Property(value) => screen_keys_in_value(value, at),
        MergeValue::ListProperty(values) => values
            .iter()
            .enumerate()
            .try_for_each(|(index, value)| screen_keys_in_value(value, &extended(at, JsonPointerSegment::Index(index)))),
        MergeValue::Relationship(_)
        | MergeValue::GeoProperty(_)
        | MergeValue::VocabProperty(_)
        | MergeValue::LanguageProperty(_)
        | MergeValue::JsonProperty(_)
        | MergeValue::ListRelationship(_) => Ok(()),
    }
}

/// The key rule over one inline Linked Entity standing at `at`: its attribute
/// members, in member order.
fn screen_keys_in_entity(entity: &Entity, at: &[JsonPointerSegment]) -> Result<(), CompactKeyError> {
    let mut members: Vec<_> = entity.attribute_members().filter(|(name, _)| !is_native_json_member(name)).collect();
    members.sort_unstable_by_key(|(name, _)| *name);
    members
        .into_iter()
        .try_for_each(|(name, member)| screen_keys_in_member(member, &extended(at, segment(name))))
}

/// The key rule over one client-written value; a typed geometry holds
/// positions only and carries nothing to refuse.
fn screen_keys_in_value(value: &NgsiLdValue, at: &[JsonPointerSegment]) -> Result<(), CompactKeyError> {
    match value {
        NgsiLdValue::Json(json) => screen_keys_in_written(json, at),
        NgsiLdValue::Geometry(_) => Ok(()),
    }
}

/// The key rule over one value carried as written, standing at `at` below the
/// top level, read by the representation crate's own walk and located from
/// the top of the entity.
fn screen_keys_in_written(value: &JsonLiteral, at: &[JsonPointerSegment]) -> Result<(), CompactKeyError> {
    screen_keys_in_nested_value(value.piece()).map_err(|error| error.relocated(|path| located(at, path)))
}

/// A pointer the representation crate's walk reported from the value it was
/// handed, located instead from `at`.
fn located(at: &[JsonPointerSegment], found: &JsonPointer) -> JsonPointer {
    JsonPointer::from_segments(at.iter().chain(found.segments()).cloned().collect())
}

#[cfg(test)]
mod tests {
    use crate::compact_screening::{screen_compact_attribute, screen_compact_entity, screen_compact_patch};
    use andromeda_ngsi_ld_model::{
        attribute::{instance::AttributeValue, merge_member::MergeMember},
        entity::{Entity, EntityPatch},
    };
    use andromeda_ngsi_ld_representation::{
        attribute_type_validation::validate_fragment_attribute_types,
        compact_key_admission::{screen_keys_in_attribute, screen_keys_in_fragment},
    };
    use serde_json::{Value, json};

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

    /// The decode refuses every payload the attribute-type rule refuses on
    /// the compact JSON, and the typed key rule reaches the same verdict at the
    /// same location as the rule over the compact JSON, for every entity of the
    /// corpus the decode admits. The decode is the stricter of the two: it reads
    /// a sub-attribute named `name` as the attribute clause 4.5.2.2 makes it,
    /// seals included, where the type rule steps over that member.
    #[test]
    fn the_decode_and_the_typed_screening_find_what_the_rules_find_on_the_compact_json() {
        for written in corpus() {
            // clone: the expectation names the payload
            if let Ok(entity) = serde_json::from_value::<Entity>(written.clone()) {
                assert!(
                    validate_fragment_attribute_types(&written).is_ok(),
                    "the decode admitted what the type rule refuses: {written}"
                );
                pretty_assertions::assert_eq!(screen_compact_entity(&entity), screen_keys_in_fragment(&written), "{written}");
            }
        }
    }

    /// An entity Fragment, and a merge fragment, are decoded and screened as
    /// the entity they patch, their three geo-properties included.
    #[test]
    fn a_fragment_is_screened_as_the_entity_it_patches() {
        for written in corpus() {
            // clone: the payload is decoded three ways
            let entity = serde_json::from_value::<Entity>(written.clone())
                .ok()
                .map(|entity| screen_compact_entity(&entity));
            let patch = serde_json::from_value::<EntityPatch>(written.clone())
                .ok()
                .map(|patch| screen_compact_patch(&patch));
            let merge = serde_json::from_value::<EntityPatch<MergeMember>>(written.clone())
                .ok()
                .map(|patch| screen_compact_patch(&patch));
            pretty_assertions::assert_eq!(patch, entity, "{written}");
            pretty_assertions::assert_eq!(merge, entity, "{written}");
        }
    }

    /// A bare Attribute Fragment is screened from the attribute's name, as
    /// the rule over its compact JSON reports it.
    #[test]
    fn an_attribute_fragment_is_screened_from_its_name() {
        let written = json!({"type": "Property", "value": {"@foo": 1}});
        let member: AttributeValue = serde_json::from_value(written.clone()).expect("a member"); // clone: the expectation names the payload
        pretty_assertions::assert_eq!(screen_compact_attribute("p", &member), screen_keys_in_attribute("p", &written));
        assert!(screen_compact_attribute("p", &member).is_err());
    }

    /// The corpus is not vacuous: it holds payloads the decode refuses and
    /// payloads the key rule refuses.
    #[test]
    fn the_corpus_holds_refusals_of_both_rules() {
        let (refused, decoded): (Vec<_>, Vec<_>) = corpus().into_iter().map(serde_json::from_value::<Entity>).partition(Result::is_err);
        let decode: Vec<String> = refused.into_iter().filter_map(Result::err).map(|error| error.to_string()).collect();
        let keys: Vec<String> = decoded
            .into_iter()
            .filter_map(Result::ok)
            .filter_map(|entity| screen_compact_entity(&entity).err())
            .map(|error| error.to_string())
            .collect();
        assert!(decode.iter().any(|finding| finding.contains("ngsildproof")), "{decode:?}");
        assert!(decode.iter().any(|finding| finding.contains("not an NGSI-LD attribute type")), "{decode:?}");
        assert!(keys.iter().any(|finding| finding.contains("form of a JSON-LD keyword")), "{keys:?}");
        assert!(keys.iter().any(|finding| finding.contains("is empty")), "{keys:?}");
    }
}
