//! One attribute instance written in a reified representation: its family's
//! members, the members every family shares, and its sub-attributes.
//!
//! What a notification reports of the instance (clause 5.8.6) -- the Null of
//! a deletion, a `deletedAt`, a previous value -- is written in place of, or
//! beside, the members the instance carries. A clause 4.15 `lang` filter turns
//! a top-level `LanguageProperty` into the `Property` it negotiates.

use crate::{
    concise::InstanceStanding,
    entity_body::{
        family_value::FamilyValue,
        linked_entities::{list_relationship_entities, relationship_entities},
        member_walk::MemberWalk,
        notified_member::{InstanceReport, MemberInstances, NullValue, previous_value_of},
        reified_member::{Reification, ReifiedMember},
        value_body::ValueBody,
    },
    language_negotiation::{NegotiatedEntry, negotiated_entry},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, rfc3339::sub_second};
use andromeda_ngsi_ld_model::attribute::{
    common_fields::CommonAttributeFields,
    geo_property_value::GeoPropertyValue,
    instance::{Attribute, AttributeValue},
};
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::{Serialize, Serializer, ser::SerializeMap};

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
    /// The bare value clause 4.5.2.3 collapses the instance to: a Property or
    /// `GeoProperty` standing alone that carries nothing beside its value, and
    /// whose value, written bare, reads back as the value of its own family --
    /// the clause calls the representation "lossless". Among the instances of a
    /// multi-attribute every instance stays an object, as the same clause
    /// writes them. The NGSI-LD Null a reported deletion writes in place of the
    /// value is the value of no family.
    fn bare_value(&self) -> Option<BareValue<'a>> {
        if self.standing == InstanceStanding::AmongInstances || self.report.deleted_at.is_some() || self.previous_member().is_some() {
            return None;
        }
        let bare = |value| Some(self.report.null.map_or(value, BareValue::Null));
        let reads_back = |value: ValueBody<'_>| self.report.null.is_some() || value.bare_family() == Some(AttributeType::Property);
        match self.attribute {
            Attribute::Property(property)
                if property.unit_code.is_none()
                    && property.value_type.is_none()
                    && carries_nothing(&property.common)
                    && reads_back(ValueBody(&property.value)) =>
            {
                bare(BareValue::Value(ValueBody(&property.value)))
            }
            Attribute::GeoProperty(geo_property) if geo_property.value_type.is_none() && carries_nothing(&geo_property.common) => {
                bare(BareValue::Geometry(&geo_property.value))
            }
            Attribute::Property(_)
            | Attribute::GeoProperty(_)
            | Attribute::Relationship(_)
            | Attribute::VocabProperty(_)
            | Attribute::LanguageProperty(_)
            | Attribute::JsonProperty(_)
            | Attribute::ListProperty(_)
            | Attribute::ListRelationship(_) => None,
        }
    }

    /// The previous member the instance is written with: the one a
    /// notification reports, or else the one the instance carries.
    ///
    /// A reported previous instance gives its value only when it spells the
    /// value member of this instance's family: clause 5.6.19 lets a change
    /// replace an attribute's family, and each family prohibits the other
    /// families' previous members (clauses 4.5.2.2 and 4.5.21.2), so a value
    /// in another family's spelling is no previous value of this one.
    fn previous_member(&self) -> Option<FamilyValue<'a>> {
        let (reification, decoding) = (self.reification, self.walk.decoding);
        match self
            .report
            .previous
            .and_then(|previous| previous_value_of(previous, self.attribute.attribute_type()))
        {
            Some(previous) => Some(FamilyValue::current(previous, reification, decoding)),
            None => FamilyValue::previous(self.attribute, reification, decoding),
        }
    }

    /// Whether the instance names its family in `type`: always in the
    /// normalized representation, and in the concise one when the typeless
    /// object would be read back as another family -- a Property holding a
    /// geometry would be taken for a `GeoProperty` (clause 5.2.5). Clause
    /// 4.5.2.3 lists `type` among the Optional members of the concise object
    /// form. The NGSI-LD Null a reported deletion writes in place of the value
    /// is the value of no family and leaves the concise form typeless.
    fn declares_type(&self) -> bool {
        match self.reification {
            Reification::Normalized => true,
            Reification::Concise => self.report.null.is_none() && self.typeless_family() != self.attribute.attribute_type(),
        }
    }

    /// The family the instance's typeless object reads back as. Every family
    /// but a Property is named by its own value member; a Property's `value`
    /// names a `GeoProperty` when it holds a geometry.
    fn typeless_family(&self) -> AttributeType {
        match self.attribute {
            Attribute::Property(property) => ValueBody(&property.value).typeless_family(),
            Attribute::GeoProperty(_)
            | Attribute::Relationship(_)
            | Attribute::VocabProperty(_)
            | Attribute::LanguageProperty(_)
            | Attribute::JsonProperty(_)
            | Attribute::ListProperty(_)
            | Attribute::ListRelationship(_) => self.attribute.attribute_type(),
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
        if self.reification == Reification::Concise
            && let Some(bare) = self.bare_value()
        {
            return bare.serialize(serializer);
        }
        let mut object = serializer.serialize_map(None)?;
        if self.declares_type() {
            object.serialize_entry("type", &self.attribute.attribute_type())?;
        }
        self.serialize_family_members(&mut object)?;
        serialize_common_members(&mut object, self.attribute.common(), self.report.deleted_at)?;
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
        let family = self.attribute.attribute_type();
        match self.report.null {
            Some(null) => object.serialize_entry(family.compact_value_key(), &null)?,
            None => object.serialize_entry(
                family.compact_value_key(),
                &FamilyValue::current(self.attribute, self.reification, self.walk.decoding),
            )?,
        }
        self.serialize_qualifiers(object)?;
        serialize_present(object, family.previous_key(), self.previous_member())?;
        if self.report.null.is_some() {
            return Ok(());
        }
        let format = self.reification.format();
        match self.attribute {
            Attribute::Relationship(relationship) => {
                serialize_present(object, compact_properties::ENTITY, relationship_entities(relationship, format, self.walk))
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

    /// Write the members that qualify the family's value: its unit, its
    /// value type and its target type.
    fn serialize_qualifiers<M: SerializeMap>(&self, object: &mut M) -> Result<(), M::Error> {
        match self.attribute {
            Attribute::Property(property) => {
                serialize_present(object, compact_properties::UNIT_CODE, property.unit_code.as_ref())?;
                serialize_present(object, compact_properties::VALUE_TYPE, property.value_type.as_ref())
            }
            Attribute::Relationship(relationship) => serialize_present(object, compact_properties::OBJECT_TYPE, relationship.object_type.as_ref()),
            Attribute::ListRelationship(list_relationship) => {
                serialize_present(object, compact_properties::OBJECT_TYPE, list_relationship.object_type.as_ref())
            }
            Attribute::VocabProperty(vocab_property) => serialize_present(object, compact_properties::VALUE_TYPE, vocab_property.value_type.as_ref()),
            Attribute::GeoProperty(geo_property) => serialize_present(object, compact_properties::VALUE_TYPE, geo_property.value_type.as_ref()),
            Attribute::LanguageProperty(language_property) => serialize_present(object, compact_properties::VALUE_TYPE, language_property.value_type.as_ref()),
            Attribute::JsonProperty(json_property) => serialize_present(object, compact_properties::VALUE_TYPE, json_property.value_type.as_ref()),
            Attribute::ListProperty(list_property) => serialize_present(object, compact_properties::VALUE_TYPE, list_property.value_type.as_ref()),
        }
    }
}

/// What clause 4.5.2.3 collapses a bare instance to.
enum BareValue<'a> {
    /// A Property's value.
    Value(ValueBody<'a>),
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

/// The `Property` a negotiated `LanguageProperty` is answered as (clause 4.15):
/// the selected entry as its value, the tag as `lang`, and of everything else
/// the attribute carries only its `observedAt`, its `datasetId` and its
/// sub-attributes.
struct NegotiatedInstance<'a> {
    /// The entry negotiation selected.
    entry: NegotiatedEntry<'a>,
    /// The members the language property carries beside its map.
    common: &'a CommonAttributeFields,
    /// The representation it is written in.
    reification: Reification,
    /// The walk its sub-attributes are written under.
    walk: MemberWalk<'a>,
}

impl Serialize for NegotiatedInstance<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut object = serializer.serialize_map(None)?;
        if self.reification == Reification::Normalized {
            object.serialize_entry("type", &AttributeType::Property)?;
        }
        object.serialize_entry(compact_properties::VALUE, self.entry.value)?;
        object.serialize_entry(compact_properties::LANG, &self.entry.lang)?;
        serialize_present(&mut object, compact_properties::OBSERVED_AT, self.common.observed_at.map(sub_second))?;
        serialize_present(&mut object, compact_properties::DATASET_ID, self.common.dataset_id.as_ref())?;
        serialize_sub_attributes(&mut object, &self.common.sub_attributes, self.reification, self.walk)?;
        object.end()
    }
}

/// Whether an instance carries nothing beside its family's own members.
fn carries_nothing(common: &CommonAttributeFields) -> bool {
    common.created_at.is_none()
        && common.dataset_id.is_none()
        && common.deleted_at.is_none()
        && common.expires_at.is_none()
        && common.instance_id.is_none()
        && common.modified_at.is_none()
        && common.observed_at.is_none()
        && common.sub_attributes.is_empty()
}

/// Write the members every attribute family shares, `deletedAt` the one a
/// notification reports when it reports one.
fn serialize_common_members<M: SerializeMap>(
    object: &mut M,
    common: &CommonAttributeFields,
    reported_deleted_at: Option<DateTime<Utc>>,
) -> Result<(), M::Error> {
    serialize_present(object, compact_properties::CREATED_AT, common.created_at.as_ref())?;
    serialize_present(object, compact_properties::DATASET_ID, common.dataset_id.as_ref())?;
    match reported_deleted_at {
        Some(deleted_at) => object.serialize_entry(compact_properties::DELETED_AT, &sub_second(deleted_at))?,
        None => serialize_present(object, compact_properties::DELETED_AT, common.deleted_at.as_ref())?,
    }
    serialize_present(object, compact_properties::EXPIRES_AT, common.expires_at.as_ref())?;
    serialize_present(object, compact_properties::INSTANCE_ID, common.instance_id.as_ref())?;
    serialize_present(object, compact_properties::MODIFIED_AT, common.modified_at.as_ref())?;
    serialize_present(object, compact_properties::OBSERVED_AT, common.observed_at.as_ref())
}

/// Write an instance's sub-attributes beside its own members.
fn serialize_sub_attributes<M: SerializeMap>(
    object: &mut M,
    sub_attributes: &IndexMap<AttributeName, AttributeValue>,
    reification: Reification,
    walk: MemberWalk<'_>,
) -> Result<(), M::Error> {
    sub_attributes
        .iter()
        .try_for_each(|(name, value)| object.serialize_entry(name.as_str(), &ReifiedMember::new(MemberInstances::Held(value), reification, walk)))
}

/// Write a member when the instance carries it.
fn serialize_present<M: SerializeMap, T: Serialize>(object: &mut M, name: &str, value: Option<T>) -> Result<(), M::Error> {
    value.map_or(Ok(()), |value| object.serialize_entry(name, &value))
}
