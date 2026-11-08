//! One attribute family's value member, and the previous member that shadows
//! it, in a reified representation.
//!
//! Every family names its value differently (clauses 4.5.2 to 4.5.24), and its
//! `showChanges` previous member has "the same representation" as that value
//! (clauses 4.5.21.2 and 4.5.22.2 say so of the lists, and the other families
//! read alike), so one writer serves both, whichever instance the previous
//! value is read from.

use crate::{
    entity_body::{
        object_list_body::ObjectListBody,
        reified_member::Reification,
        value_body::{ValueBody, ValueListBody},
    },
    version::list_as_relationship::ListAsRelationship,
};
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, coerced_term::CoercedTerm, uri::Uri};
use andromeda_ngsi_ld_model::attribute::{geo_property_value::GeoPropertyValue, instance::Attribute, language_property::LanguageMap, one_or_many::OneOrMany};
use serde::{Serialize, Serializer};

/// One family's value, written as its value member or its previous member.
#[derive(Debug, Clone, Copy)]
pub(crate) enum FamilyValue<'a> {
    /// A Property's value.
    Value(ValueBody<'a>),
    /// A Relationship's target or targets, as written (clause 4.5.3.2).
    Targets(&'a OneOrMany<Uri>),
    /// A `VocabProperty`'s vocabulary terms.
    Vocab(&'a OneOrMany<CoercedTerm>),
    /// A `GeoProperty`'s geometry.
    Geometry(&'a GeoPropertyValue),
    /// A `LanguageProperty`'s language map.
    LanguageMap(&'a LanguageMap),
    /// A `JsonProperty`'s JSON value.
    Json(&'a JsonLiteral),
    /// A `ListProperty`'s values.
    ValueList(ValueListBody<'a>),
    /// A `ListRelationship`'s targets.
    ObjectList(ObjectListBody<'a>),
    /// A `ListRelationship`'s targets, written as the `object` of the
    /// Relationship Table 4.3.6.8-1 reformats it as.
    ListAsRelationship(ListAsRelationship<'a>),
}

impl<'a> FamilyValue<'a> {
    /// The value member `attribute` carries.
    pub(crate) fn current(attribute: &'a Attribute, reification: Reification) -> FamilyValue<'a> {
        match attribute {
            Attribute::Property(property) => FamilyValue::Value(ValueBody(&property.value)),
            Attribute::Relationship(relationship) => FamilyValue::Targets(&relationship.object),
            Attribute::VocabProperty(vocab_property) => FamilyValue::Vocab(&vocab_property.vocab),
            Attribute::GeoProperty(geo_property) => FamilyValue::Geometry(&geo_property.value),
            Attribute::LanguageProperty(language_property) => FamilyValue::LanguageMap(&language_property.language_map),
            Attribute::JsonProperty(json_property) => FamilyValue::Json(&json_property.json),
            Attribute::ListProperty(list_property) => FamilyValue::ValueList(ValueListBody(&list_property.value_list)),
            Attribute::ListRelationship(list_relationship) => FamilyValue::ObjectList(ObjectListBody::new(&list_relationship.object_list, reification)),
        }
    }

    /// The value, as the member of `family` writes it once Table 4.3.6.8-1
    /// reformats the family that carries it: a `ListRelationship`'s list
    /// becomes the `object` of a Relationship, and every other value
    /// moves under its generalization's key unchanged.
    pub(crate) fn written_as(self, family: AttributeType) -> FamilyValue<'a> {
        match self {
            FamilyValue::ObjectList(list) if family == AttributeType::Relationship => FamilyValue::ListAsRelationship(list.as_relationship()),
            unchanged @ (FamilyValue::Value(_)
            | FamilyValue::Targets(_)
            | FamilyValue::Vocab(_)
            | FamilyValue::Geometry(_)
            | FamilyValue::LanguageMap(_)
            | FamilyValue::Json(_)
            | FamilyValue::ValueList(_)
            | FamilyValue::ObjectList(_)
            | FamilyValue::ListAsRelationship(_)) => unchanged,
        }
    }

    /// The previous member `attribute` itself carries, if any.
    pub(crate) fn previous(attribute: &'a Attribute, reification: Reification) -> Option<FamilyValue<'a>> {
        match attribute {
            Attribute::Property(property) => property.previous_value.as_ref().map(|previous| FamilyValue::Value(ValueBody(previous))),
            Attribute::Relationship(relationship) => relationship.previous_object.as_ref().map(FamilyValue::Targets),
            Attribute::VocabProperty(vocab_property) => vocab_property.previous_vocab.as_ref().map(FamilyValue::Vocab),
            Attribute::GeoProperty(geo_property) => geo_property.previous_value.as_ref().map(FamilyValue::Geometry),
            Attribute::LanguageProperty(language_property) => language_property.previous_language_map.as_ref().map(FamilyValue::LanguageMap),
            Attribute::JsonProperty(json_property) => json_property.previous_json.as_ref().map(FamilyValue::Json),
            Attribute::ListProperty(list_property) => list_property
                .previous_value_list
                .as_deref()
                .map(|previous| FamilyValue::ValueList(ValueListBody(previous))),
            Attribute::ListRelationship(list_relationship) => list_relationship
                .previous_object_list
                .as_ref()
                .map(|previous| FamilyValue::ObjectList(ObjectListBody::new(previous, reification))),
        }
    }
}

impl Serialize for FamilyValue<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            FamilyValue::Value(value) => value.serialize(serializer),
            FamilyValue::Targets(targets) => targets.serialize(serializer),
            FamilyValue::Vocab(vocab) => vocab.serialize(serializer),
            FamilyValue::Geometry(geometry) => geometry.serialize(serializer),
            FamilyValue::LanguageMap(language_map) => language_map.serialize(serializer),
            FamilyValue::Json(json) => json.serialize(serializer),
            FamilyValue::ValueList(values) => values.serialize(serializer),
            FamilyValue::ObjectList(list) => list.serialize(serializer),
            FamilyValue::ListAsRelationship(object) => object.serialize(serializer),
        }
    }
}
