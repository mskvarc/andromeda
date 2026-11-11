//! The requirements an attribute matcher makes of one attribute instance, and how the instance is checked against
//! each: payloads, instants and numbers compared by [`Equivalent`], members read through the typed model.

use crate::{
    attribute_matcher::AttributeMatcher,
    attribute_member::Member,
    attribute_reading::{has_member, member_at, unit_code_of, value_of, value_type_of},
    equivalence::{Comparison, Equivalent},
};
use cefact_units::UnitCode;
use ngsi_ld::model::{
    Attribute,
    DatasetId,
    DateTime,
    EntityId,
    EntityTypeName,
    GeoValue,
    InstanceId,
    OneOrMany,
    Output,
    RawJson,
    Value,
    VocabTerm,
    geometry::Geometry,
};
use std::slice;

/// One requirement on an attribute.
#[derive(Debug, Clone)]
pub(crate) enum Check {
    /// The value of a Property, the items of a `ListProperty`.
    Value(Value),
    /// The member at a path inside the value, the value itself for an empty path, under a comparison.
    ValueMember(Vec<String>, Value, Comparison),
    /// The number of items or members at a path inside the value.
    Length(Vec<String>, usize),
    /// The number of items of a `ListProperty`'s `valueList`, a `ListRelationship`'s `objectList` or a
    /// `VocabProperty`'s array of terms.
    ListLength(usize),
    /// The item at a position of a `ListProperty`'s `valueList`.
    ListValue(usize, Value),
    /// The object at a position of a `ListRelationship`'s `objectList`.
    ListObject(usize, EntityId),
    /// A term of a `VocabProperty`, written alone or in its array.
    VocabHolding(VocabTerm),
    /// The object of a Relationship.
    Object(OneOrMany<EntityId>),
    /// The `objectType` of a Relationship.
    ObjectType(OneOrMany<VocabTerm>),
    /// The geometry of a `GeoProperty`.
    Geometry(Geometry),
    /// The vocabulary of a `VocabProperty`.
    Vocab(OneOrMany<VocabTerm>),
    /// The text of one language of a `LanguageProperty`.
    Language(String, String),
    /// The `lang` a language filter writes on the Property it picked one language of (clause 4.15).
    Lang(String),
    /// `unitCode`.
    UnitCode(UnitCode),
    /// `datasetId`, of the instance read.
    DatasetId(DatasetId),
    /// `valueType`.
    ValueType(VocabTerm),
    /// `observedAt`.
    ObservedAt(DateTime),
    /// `expiresAt`.
    ExpiresAt(DateTime),
    /// `instanceId` (clause 4.5.7).
    InstanceId(InstanceId),
    /// No member at a path inside the value.
    NoValueMember(Vec<String>),
    /// The member is absent.
    Absent(Member),
    /// The member is present.
    Present(Member),
    /// A sub-attribute meets its matcher.
    SubAttribute(String, Box<AttributeMatcher>),
    /// The sub-attribute is absent.
    NoSubAttribute(String),
    /// The number of sub-attributes.
    SubAttributeCount(usize),
    /// The number of instances of one sub-attribute, one per dataset (clause 4.5.5).
    SubAttributeInstances(String, usize),
    /// The payload is the NGSI-LD Null: a `Property`, `GeoProperty` or Relationship a broker reports deleted (clause
    /// 4.5.0).
    NullValue,
    /// The `previousValue` of a Property, which a notification with `showChanges` carries (clause 5.8.1).
    PreviousValue(Value),
    /// The `previousObject` of a Relationship, which a notification with `showChanges` carries (clause 5.8.1).
    PreviousObject(OneOrMany<EntityId>),
    /// The attribute carries an `ngsildproof`.
    Proof,
    /// The attribute carries no `ngsildproof`.
    NoProof,
    /// The `entityIdSealed` of the attribute's `ngsildproof`.
    SealedId(EntityId),
    /// The `entityTypeSealed` of the attribute's `ngsildproof`, one type.
    SealedType(EntityTypeName),
    /// The member at a path inside the proof object, the value of the attribute's `ngsildproof`.
    ProofMember(Vec<String>, Value),
    /// The whole `json` of a `JsonProperty`, compared as parsed JSON (clause 4.5.24 keeps the json opaque).
    Json(serde_json::Value),
    /// The whole `previousJson` of a `JsonProperty`, which a notification with `showChanges` carries (clause 5.8.1),
    /// compared as parsed JSON.
    PreviousJson(serde_json::Value),
}

/// Why `attribute` breaks `check`, if it does.
pub(crate) fn check_mismatch(check: &Check, attribute: &Attribute<Output>) -> Option<String> {
    let meta = attribute.meta();
    let holds = match check {
        Check::Value(expected) => value_of(attribute).is_some_and(|actual| actual.equivalent(expected)),
        Check::ValueMember(path, expected, comparison) => {
            value_of(attribute).is_some_and(|actual| member_at(&actual, path).is_some_and(|member| comparison.holds(member, expected)))
        }
        Check::Length(path, expected) => value_of(attribute).is_some_and(|actual| {
            member_at(&actual, path).is_some_and(|member| match member {
                Value::Array(items) => items.len() == *expected,
                Value::Object(members) => members.len() == *expected,
                Value::Boolean(_) | Value::Number(_) | Value::Text(_) | Value::Typed(_) | Value::JsonNull => false,
            })
        }),
        Check::ListLength(expected) => match attribute {
            Attribute::ListProperty(property) => property.value_list().len() == *expected,
            Attribute::ListRelationship(relationship) => relationship.object_list().len() == *expected,
            Attribute::VocabProperty(property) => property.vocab().is_many() && property.vocab().as_slice().len() == *expected,
            Attribute::Property(_) | Attribute::GeoProperty(_) | Attribute::LanguageProperty(_) | Attribute::JsonProperty(_) | Attribute::Relationship(_) => {
                false
            }
        },
        Check::ListValue(index, expected) => {
            matches!(attribute, Attribute::ListProperty(property) if property.value_list().get(*index).is_some_and(|item| item.equivalent(expected)))
        }
        Check::ListObject(index, expected) => {
            matches!(attribute, Attribute::ListRelationship(relationship) if relationship.object_list().get(*index) == Some(expected))
        }
        Check::VocabHolding(expected) => matches!(attribute, Attribute::VocabProperty(property) if property.vocab().iter().any(|term| term == expected)),
        Check::Object(expected) => matches!(attribute, Attribute::Relationship(relationship) if relationship.object() == expected),
        Check::ObjectType(expected) => matches!(attribute, Attribute::Relationship(relationship) if relationship.object_type() == Some(expected)),
        Check::Geometry(expected) => matches!(attribute, Attribute::GeoProperty(property) if property.value() == &GeoValue::Geometry(expected.clone())),
        Check::Vocab(expected) => matches!(attribute, Attribute::VocabProperty(property) if property.vocab() == expected),
        Check::Language(language, text) => matches!(attribute, Attribute::LanguageProperty(property)
            if property.language_map().iter().any(|(key, value)| key.to_string() == *language && *value == text.as_str().into())),
        Check::Lang(expected) => {
            matches!(attribute, Attribute::Property(property) if property.output().lang.as_ref().is_some_and(|lang| lang.to_string() == *expected))
        }
        Check::UnitCode(expected) => unit_code_of(attribute) == Some(expected),
        Check::DatasetId(expected) => meta.dataset_id.as_ref() == Some(expected),
        Check::ValueType(expected) => value_type_of(attribute) == Some(expected),
        Check::ObservedAt(expected) => meta.observed_at.is_some_and(|actual| actual.equivalent(expected)),
        Check::ExpiresAt(expected) => meta.expires_at.is_some_and(|actual| actual.equivalent(expected)),
        Check::InstanceId(expected) => meta.output.instance_id.as_ref() == Some(expected),
        Check::NoValueMember(path) => value_of(attribute).is_some_and(|actual| member_at(&actual, path).is_none()),
        Check::Absent(member) => !has_member(attribute, meta, *member),
        Check::Present(member) => has_member(attribute, meta, *member),
        Check::SubAttribute(name, matcher) => return matcher.mismatch_in(&meta.attributes, name),
        Check::NoSubAttribute(name) => meta.attributes.names().all(|candidate| candidate.as_str() != *name),
        Check::SubAttributeCount(count) => meta.attributes.len() == *count,
        Check::SubAttributeInstances(name, count) => meta
            .attributes
            .iter()
            .any(|(candidate, instances)| candidate.as_str() == *name && instances.len() == *count),
        Check::NullValue => matches!(attribute, Attribute::Property(_) | Attribute::GeoProperty(_) | Attribute::Relationship(_)) && attribute.is_deleted(),
        Check::PreviousValue(expected) => matches!(attribute, Attribute::Property(property)
            if property.output().previous_value.as_ref().is_some_and(|previous| previous.equivalent(expected))),
        Check::PreviousObject(expected) => {
            matches!(attribute, Attribute::Relationship(relationship) if relationship.output().previous_object.as_ref() == Some(expected))
        }
        Check::Proof => meta.proof.is_some(),
        Check::NoProof => meta.proof.is_none(),
        Check::SealedId(expected) => meta.proof.as_ref().is_some_and(|proof| proof.entity_id_sealed() == expected),
        Check::SealedType(expected) => meta
            .proof
            .as_ref()
            .is_some_and(|proof| proof.entity_type_sealed().as_slice() == slice::from_ref(expected)),
        Check::ProofMember(path, expected) => meta
            .proof
            .as_ref()
            .is_some_and(|proof| member_at(proof.property().value(), path).is_some_and(|member| member.equivalent(expected))),
        Check::Json(expected) => match attribute {
            Attribute::JsonProperty(property) => match json_is(property.json(), expected) {
                Some(holds) => holds,
                None => return Some(String::from("breaks the json check: its json is no JSON")),
            },
            Attribute::Property(_)
            | Attribute::ListProperty(_)
            | Attribute::GeoProperty(_)
            | Attribute::LanguageProperty(_)
            | Attribute::VocabProperty(_)
            | Attribute::Relationship(_)
            | Attribute::ListRelationship(_) => false,
        },
        Check::PreviousJson(expected) => match attribute {
            Attribute::JsonProperty(property) => match property.output().previous_json.as_ref().map(|previous| json_is(previous, expected)) {
                Some(Some(holds)) => holds,
                Some(None) => return Some(String::from("breaks the previous json check: its previousJson is no JSON")),
                None => false,
            },
            Attribute::Property(_)
            | Attribute::ListProperty(_)
            | Attribute::GeoProperty(_)
            | Attribute::LanguageProperty(_)
            | Attribute::VocabProperty(_)
            | Attribute::Relationship(_)
            | Attribute::ListRelationship(_) => false,
        },
    };
    (!holds).then(|| format!("breaks {check:?}: it is {attribute:?}"))
}

/// Whether the opaque json `raw` parses to `expected`, or `None` when it is no JSON.
fn json_is(raw: &RawJson, expected: &serde_json::Value) -> Option<bool> {
    serde_json::from_str::<serde_json::Value>(&raw.to_string())
        .ok()
        .map(|actual| actual == *expected)
}
