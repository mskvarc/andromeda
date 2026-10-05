//! One attribute instance written in a reified representation: its family's
//! members, the members every family shares, and its sub-attributes.
//!
//! What a notification reports of the instance (clause 5.8.6) -- the Null of
//! a deletion, a `deletedAt`, a previous value -- is written in place of, or
//! beside, the members the instance carries. A clause 4.15 `lang` filter turns
//! a top-level `LanguageProperty` into the `Property` it negotiates. A clause
//! 4.3.6.8 version fallback writes the instance as the family the target
//! version knows, under that family's value and previous keys, and leaves out
//! the members the target never defined.

use crate::{
    concise::InstanceStanding,
    entity_body::{
        common_members::{carries_nothing, serialize_common_members, serialize_present, serialize_sub_attributes},
        family_value::FamilyValue,
        instance_fallback::{GeneralizationError, GeneralizedValue},
        linked_entities::{first_target_entities, list_relationship_entities, relationship_entities},
        member_walk::MemberWalk,
        negotiated_instance::NegotiatedInstance,
        notified_member::{InstanceReport, NullValue, previous_value_of},
        reified_member::Reification,
        value_body::ValueBody,
    },
    language_negotiation::negotiated_entry,
    version::fallback::InstanceMember,
};
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use andromeda_ngsi_ld_model::attribute::{geo_property_value::GeoPropertyValue, instance::Attribute};
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use serde::{
    Serialize,
    Serializer,
    ser::{Error as _, SerializeMap},
};

/// One attribute instance in a reified representation.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ReifiedInstance<'a> {
    /// The instance written.
    pub(crate) attribute: &'a Attribute,
    /// The representation it is written in.
    pub(crate) reification: Reification,
    /// What the walk applies on the way.
    pub(crate) walk: MemberWalk<'a>,
    /// What a notification reports of the instance.
    pub(crate) report: InstanceReport<'a>,
    /// Where it stands in the member it is written in.
    pub(crate) standing: InstanceStanding,
}

impl<'a> ReifiedInstance<'a> {
    /// The family the instance is written as: its own, or the one a version
    /// fallback reformats it as.
    const fn written_family(&self) -> AttributeType {
        self.walk.written_family(self.attribute.attribute_type())
    }

    /// Whether the instance keeps `member` under the version fallback.
    const fn keeps(&self, member: InstanceMember) -> bool {
        self.walk.keeps_instance_member(self.written_family(), member)
    }

    /// The NGSI-LD Null a reported deletion writes in place of the value.
    const fn null(&self) -> Option<NullValue> {
        self.report.null(self.written_family())
    }

    /// The value member the instance carries, as its written family writes it.
    fn current(&self) -> FamilyValue<'a> {
        FamilyValue::current(self.attribute, self.reification, self.walk.decoding).written_as(self.written_family())
    }

    /// The value of a family a version fallback reformats as a Property, read
    /// back the way clause 4.5.2.3 reads a Property value; `None` for every
    /// instance written as its own family, and for the Relationship a
    /// `ListRelationship` becomes, whose `object` names its family.
    fn generalized_value(&self) -> Result<Option<GeneralizedValue>, GeneralizationError> {
        let written = self.written_family();
        if written == self.attribute.attribute_type() || written != AttributeType::Property {
            return Ok(None);
        }
        GeneralizedValue::of(&self.current()).map(Some)
    }

    /// The bare value clause 4.5.2.3 collapses the instance to: a Property or
    /// `GeoProperty` standing alone that carries nothing beside its value, and
    /// whose value, written bare, reads back as the value of its own family --
    /// the clause calls the representation "lossless". Among the instances of a
    /// multi-attribute every instance stays an object, as the same clause
    /// writes them. The NGSI-LD Null a reported deletion writes in place of the
    /// value is the value of no family. A member a version fallback leaves out
    /// is not carried, and a family it reformats as a Property collapses as a
    /// Property does.
    fn bare_value(&self, generalized: Option<&GeneralizedValue>) -> Option<BareValue<'a>> {
        if self.standing == InstanceStanding::AmongInstances
            || self.report.deleted_at.is_some()
            || self.previous_member().is_some()
            || self.carries_qualifier()
            || !carries_nothing(self.attribute.common(), |member| self.keeps(member))
        {
            return None;
        }
        let null = self.null();
        let reads_back = |family: Option<AttributeType>| null.is_some() || family == Some(AttributeType::Property);
        let value = match self.attribute {
            Attribute::Property(property) => {
                reads_back(ValueBody(&property.value).bare_family()).then_some(BareValue::Value(FamilyValue::Value(ValueBody(&property.value))))
            }
            Attribute::GeoProperty(geo_property) => Some(BareValue::Geometry(&geo_property.value)),
            Attribute::VocabProperty(_) | Attribute::LanguageProperty(_) | Attribute::JsonProperty(_) | Attribute::ListProperty(_) => generalized
                .filter(|generalized| reads_back(generalized.bare_family()))
                .map(|_| BareValue::Value(self.current())),
            Attribute::Relationship(_) | Attribute::ListRelationship(_) => None,
        }?;
        Some(null.map_or(value, BareValue::Null))
    }

    /// Whether the instance carries a `unitCode` or a `valueType` the version
    /// fallback keeps, which a bare value has nowhere to write.
    const fn carries_qualifier(&self) -> bool {
        let (unit_code, value_type) = match self.attribute {
            Attribute::Property(property) => (property.unit_code.is_some(), property.value_type.is_some()),
            Attribute::GeoProperty(geo_property) => (false, geo_property.value_type.is_some()),
            Attribute::VocabProperty(vocab_property) => (false, vocab_property.value_type.is_some()),
            Attribute::LanguageProperty(language_property) => (false, language_property.value_type.is_some()),
            Attribute::JsonProperty(json_property) => (false, json_property.value_type.is_some()),
            Attribute::ListProperty(list_property) => (false, list_property.value_type.is_some()),
            Attribute::Relationship(_) | Attribute::ListRelationship(_) => (false, false),
        };
        (unit_code && self.keeps(InstanceMember::UnitCode)) || (value_type && self.keeps(InstanceMember::ValueType))
    }

    /// The previous member the instance is written with: the one a
    /// notification reports, or else the one the instance carries, as its
    /// written family writes it.
    ///
    /// A reported previous instance gives its value only when it spells the
    /// value member of this instance's family: clause 5.6.19 lets a change
    /// replace an attribute's family, and each family prohibits the other
    /// families' previous members (clauses 4.5.2.2 and 4.5.21.2), so a value
    /// in another family's spelling is no previous value of this one.
    fn previous_member(&self) -> Option<FamilyValue<'a>> {
        let (reification, decoding, written) = (self.reification, self.walk.decoding, self.written_family());
        match self
            .report
            .previous
            .and_then(|previous| previous_value_of(previous, self.attribute.attribute_type()))
        {
            Some(previous) => Some(FamilyValue::current(previous, reification, decoding).written_as(written)),
            None => FamilyValue::previous(self.attribute, reification, decoding).map(|previous| previous.written_as(written)),
        }
    }

    /// Whether the instance names its family in `type`: always in the
    /// normalized representation, and in the concise one when the typeless
    /// object would be read back as another family -- a Property holding a
    /// geometry would be taken for a `GeoProperty` (clause 5.2.5). Clause
    /// 4.5.2.3 lists `type` among the Optional members of the concise object
    /// form. The NGSI-LD Null a reported deletion writes in place of the value
    /// is the value of no family and leaves the concise form typeless.
    fn declares_type(&self, generalized: Option<&GeneralizedValue>) -> bool {
        match self.reification {
            Reification::Normalized => true,
            Reification::Concise => self.null().is_none() && self.typeless_family(generalized) != self.written_family(),
        }
    }

    /// The family the instance's typeless object reads back as. Every family
    /// but a Property is named by its own value member; a Property's `value`
    /// names a `GeoProperty` when it holds a geometry, and so does the value a
    /// version fallback moved under `value`.
    fn typeless_family(&self, generalized: Option<&GeneralizedValue>) -> AttributeType {
        match self.attribute {
            Attribute::Property(property) => ValueBody(&property.value).typeless_family(),
            Attribute::VocabProperty(_) | Attribute::LanguageProperty(_) | Attribute::JsonProperty(_) | Attribute::ListProperty(_) => {
                generalized.map_or(self.written_family(), GeneralizedValue::typeless_family)
            }
            Attribute::GeoProperty(_) | Attribute::Relationship(_) | Attribute::ListRelationship(_) => self.written_family(),
        }
    }

    /// The walk the instance's sub-attributes are written under: the same,
    /// except that clause 4.15 negotiates, and clause 4.5.23 nests the targets
    /// of, top-level attributes only.
    const fn nested(&self) -> MemberWalk<'a> {
        self.walk.nested()
    }
}

impl Serialize for ReifiedInstance<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if let (Some(filter), Attribute::LanguageProperty(language_property)) = (self.walk.language_filter, self.attribute)
            && let Some(entry) = negotiated_entry(language_property, filter)
        {
            return NegotiatedInstance {
                entry,
                common: &language_property.common,
                reification: self.reification,
                walk: self.nested(),
            }
            .serialize(serializer);
        }
        let generalized = match self.reification {
            Reification::Concise => self.generalized_value().map_err(S::Error::custom)?,
            Reification::Normalized => None,
        };
        if self.reification == Reification::Concise
            && let Some(bare) = self.bare_value(generalized.as_ref())
        {
            return bare.serialize(serializer);
        }
        let mut object = serializer.serialize_map(None)?;
        if self.declares_type(generalized.as_ref()) {
            object.serialize_entry("type", &self.written_family())?;
        }
        self.serialize_family_members(&mut object)?;
        serialize_common_members(&mut object, self.attribute.common(), self.report.deleted_at, |member| self.keeps(member))?;
        serialize_sub_attributes(&mut object, &self.attribute.common().sub_attributes, self.reification, self.nested())?;
        object.end()
    }
}

impl ReifiedInstance<'_> {
    /// Write the members the instance's family carries beside the shared ones:
    /// its value -- or the Null a reported deletion puts in its place -- the
    /// members qualifying it, its previous value, and the Linked Entities an
    /// inline join nests under a Relationship that still names its targets.
    fn serialize_family_members<M: SerializeMap>(&self, object: &mut M) -> Result<(), M::Error> {
        let family = self.written_family();
        match self.null() {
            Some(null) => object.serialize_entry(family.compact_value_key(), &null)?,
            None => object.serialize_entry(family.compact_value_key(), &self.current())?,
        }
        self.serialize_qualifiers(object)?;
        serialize_present(object, family.previous_key(), self.previous_member())?;
        if self.null().is_some() {
            return Ok(());
        }
        let format = self.reification.format();
        match self.attribute {
            Attribute::Relationship(relationship) => {
                serialize_present(object, compact_properties::ENTITY, relationship_entities(relationship, format, self.walk))
            }
            // A `ListRelationship` reformatted as a Relationship nests the
            // entity its one remaining target names, as a Relationship does.
            Attribute::ListRelationship(list_relationship) if family == AttributeType::Relationship => {
                serialize_present(object, compact_properties::ENTITY, first_target_entities(list_relationship, format, self.walk))
            }
            Attribute::ListRelationship(list_relationship) => serialize_present(
                object,
                compact_properties::ENTITY_LIST,
                list_relationship_entities(list_relationship, format, self.walk),
            ),
            Attribute::Property(_)
            | Attribute::VocabProperty(_)
            | Attribute::GeoProperty(_)
            | Attribute::LanguageProperty(_)
            | Attribute::JsonProperty(_)
            | Attribute::ListProperty(_) => Ok(()),
        }
    }

    /// Write the members that qualify the family's value -- its unit, its
    /// value type and its target type -- that the version fallback keeps.
    fn serialize_qualifiers<M: SerializeMap>(&self, object: &mut M) -> Result<(), M::Error> {
        let value_type = self.keeps(InstanceMember::ValueType);
        let object_type = self.keeps(InstanceMember::ObjectType);
        match self.attribute {
            Attribute::Property(property) => {
                serialize_present(
                    object,
                    compact_properties::UNIT_CODE,
                    property.unit_code.as_ref().filter(|_| self.keeps(InstanceMember::UnitCode)),
                )?;
                serialize_present(object, compact_properties::VALUE_TYPE, property.value_type.as_ref().filter(|_| value_type))
            }
            Attribute::Relationship(relationship) => serialize_present(
                object,
                compact_properties::OBJECT_TYPE,
                relationship.object_type.as_ref().filter(|_| object_type),
            ),
            Attribute::ListRelationship(list_relationship) => serialize_present(
                object,
                compact_properties::OBJECT_TYPE,
                list_relationship.object_type.as_ref().filter(|_| object_type),
            ),
            Attribute::VocabProperty(vocab_property) => serialize_present(
                object,
                compact_properties::VALUE_TYPE,
                vocab_property.value_type.as_ref().filter(|_| value_type),
            ),
            Attribute::GeoProperty(geo_property) => {
                serialize_present(object, compact_properties::VALUE_TYPE, geo_property.value_type.as_ref().filter(|_| value_type))
            }
            Attribute::LanguageProperty(language_property) => serialize_present(
                object,
                compact_properties::VALUE_TYPE,
                language_property.value_type.as_ref().filter(|_| value_type),
            ),
            Attribute::JsonProperty(json_property) => {
                serialize_present(object, compact_properties::VALUE_TYPE, json_property.value_type.as_ref().filter(|_| value_type))
            }
            Attribute::ListProperty(list_property) => {
                serialize_present(object, compact_properties::VALUE_TYPE, list_property.value_type.as_ref().filter(|_| value_type))
            }
        }
    }
}

/// What clause 4.5.2.3 collapses a bare instance to.
enum BareValue<'a> {
    /// A Property's value, or the value of a family reformatted as one.
    Value(FamilyValue<'a>),
    /// A `GeoProperty`'s geometry.
    Geometry(&'a GeoPropertyValue),
    /// The Null a reported deletion puts in place of either.
    Null(NullValue),
}

impl Serialize for BareValue<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            BareValue::Value(value) => value.serialize(serializer),
            BareValue::Geometry(geometry) => geometry.serialize(serializer),
            BareValue::Null(null) => null.serialize(serializer),
        }
    }
}
