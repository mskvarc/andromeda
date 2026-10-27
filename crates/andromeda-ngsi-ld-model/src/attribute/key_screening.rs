//! The clause 4.6.4 key rule, read off the typed compact attribute members.
//!
//! Clause 4.6.4 forbids a key JSON-LD expansion would drop or rewrite past
//! recovery. Clause 5.5.7's `@context` below the top level is not among them:
//! it is part of the request's user `@context`, refused by the user `@context`
//! admission before this screening runs. The attribute types of clause 5.2.4
//! and the seals of clause 4.5.2.2 are not among them either: the decode of the
//! request refuses a node either rule refuses, so a decoded payload holds
//! none. The rule is applied to what the decode holds, member by member,
//! rather than to a JSON copy of the whole entity rendered back for it:
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

use crate::{
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
    entity::Entity,
};
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_number::json_value::json_pointer::{JsonPointer, JsonPointerSegment};
use andromeda_ngsi_ld_representation::{
    compact_key_admission::{CompactKeyError, screen_keys_in_nested_value},
    embedded_context_admission::is_native_json_member,
};
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;

/// One member name as a pointer segment.
#[must_use]
pub fn member_segment(name: &str) -> JsonPointerSegment {
    JsonPointerSegment::Member(Box::from(name))
}

/// `prefix` followed by one more segment.
fn extended(prefix: &[JsonPointerSegment], next: JsonPointerSegment) -> Vec<JsonPointerSegment> {
    prefix.iter().cloned().chain([next]).collect()
}

/// The key rule over one attribute member standing at `at`; the NGSI-LD Null
/// carries no key.
///
/// # Errors
///
/// Returns the first key clause 4.6.4 refuses.
pub fn screen_keys_in_member(member: &AttributeValue, at: &[JsonPointerSegment]) -> Result<(), CompactKeyError> {
    match member {
        AttributeValue::Single(instance) => screen_keys_in_instance(instance, at),
        AttributeValue::Multiple(instances) => screen_keys_in_instances(instances, at),
        AttributeValue::NgsiLdNull => Ok(()),
    }
}

/// The key rule over the instances of one attribute standing at `at`, each
/// instance in order.
///
/// # Errors
///
/// Returns the first key clause 4.6.4 refuses.
pub fn screen_keys_in_instances(instances: &[Attribute], at: &[JsonPointerSegment]) -> Result<(), CompactKeyError> {
    instances
        .iter()
        .enumerate()
        .try_for_each(|(index, instance)| screen_keys_in_instance(instance, &extended(at, JsonPointerSegment::Index(index))))
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
        let at = extended(at, member_segment(name));
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
///
/// # Errors
///
/// Returns the first key clause 4.6.4 refuses.
pub fn screen_keys_in_merge_member(member: &MergeMember, at: &[JsonPointerSegment]) -> Result<(), CompactKeyError> {
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
        let at = extended(at, member_segment(name));
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
        .try_for_each(|(name, member)| screen_keys_in_member(member, &extended(at, member_segment(name))))
}

/// The key rule over one client-written value.
fn screen_keys_in_value(value: &NgsiLdValue, at: &[JsonPointerSegment]) -> Result<(), CompactKeyError> {
    screen_keys_in_written(value.literal(), at)
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
