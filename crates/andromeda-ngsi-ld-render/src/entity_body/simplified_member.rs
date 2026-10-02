//! An attribute member written in the clause 4.5.4 simplified (`keyValues`)
//! representation.
//!
//! A single instance becomes its value -- wrapped in its one distinguishing
//! member for a `LanguageProperty`, `JsonProperty` and `VocabProperty`, and
//! reduced to the bare URIs for a `ListRelationship` -- and a multi-attribute
//! becomes the `dataset` map keyed by each instance's `datasetId`, `@none` for
//! the default. A clause 4.15 `lang` filter reduces a top-level
//! `LanguageProperty` to the text it negotiates, and a Relationship that nests
//! Linked Entities (clause 4.5.23) answers with them rather than with the URIs
//! they were resolved from (clause 4.5.4, EXAMPLE 9 and EXAMPLE 10).

use crate::{
    entity_body::{
        geometry_body::GeometryBody,
        linked_entities::{list_relationship_entities, relationship_entities},
        member_walk::MemberWalk,
        notified_member::{InstanceReport, InstanceSource, MemberInstances, NotifiedInstance, NotifiedMember, NullValue, reported_verbatim},
        object_list_body::ObjectListBody,
        reified_member::Reification,
        value_body::{ValueBody, ValueListBody},
    },
    language_negotiation::negotiated_entry,
    simplify::{simplify_attr_value, simplify_raw_envelope},
    uri_decoding::{DecodedTargets, decoded_attribute},
};
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use andromeda_ngsi_ld_model::attribute::instance::{Attribute, AttributeValue};
use andromeda_ngsi_ld_query::dataset::DEFAULT_INSTANCE_TOKEN;
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use serde::{Serialize, Serializer, ser::SerializeMap};

/// Which of the two clause 4.5.4 readings a member carried verbatim takes.
///
/// The simplified rendering of a typed entity and that of an already-rendered
/// document agree on every typed instance and differ on a member the broker
/// carries verbatim: the entity reading simplifies only an attribute envelope
/// and keeps anything else as it stands, while the document reading also
/// gathers an array of instances into a `dataset` map. A request that asks for
/// nothing beyond the representation is answered by the first, one that also
/// negotiates a language or projects members by the second.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VerbatimReading {
    /// The typed entity's reading.
    Envelope,
    /// The rendered document's reading.
    Document,
}

/// One attribute member in the simplified representation.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SimplifiedMember<'a> {
    /// The member written.
    value: MemberInstances<'a>,
    /// What the walk applies on the way.
    walk: MemberWalk<'a>,
    /// How a member carried verbatim is read.
    verbatim: VerbatimReading,
}

impl<'a> SimplifiedMember<'a> {
    /// The member, written under `walk`, a verbatim one read by `verbatim`.
    pub(crate) const fn new(value: MemberInstances<'a>, walk: MemberWalk<'a>, verbatim: VerbatimReading) -> SimplifiedMember<'a> {
        SimplifiedMember { value, walk, verbatim }
    }

    /// One instance with what is reported of it.
    const fn instance(&self, source: InstanceSource<'a>, report: InstanceReport<'a>) -> SimplifiedInstance<'a> {
        SimplifiedInstance {
            source,
            report,
            walk: self.walk,
            verbatim: self.verbatim,
        }
    }
}

impl Serialize for SimplifiedMember<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let held = InstanceReport::default();
        match self.value {
            MemberInstances::Held(AttributeValue::Single(attribute)) => self.instance(InstanceSource::Typed(attribute), held).serialize(serializer),
            MemberInstances::Held(AttributeValue::Multiple(instances)) => dataset_map(serializer, DatasetInstances::Held(instances), self),
            MemberInstances::Held(AttributeValue::Raw(raw)) => self.instance(InstanceSource::Verbatim(raw), held).serialize(serializer),
            MemberInstances::Notified(NotifiedMember::One(instance)) => self.instance(instance.source, instance.report).serialize(serializer),
            MemberInstances::Notified(NotifiedMember::Many(instances)) => dataset_map(serializer, DatasetInstances::Notified(instances), self),
        }
    }
}

/// The instances of a multi-attribute.
#[derive(Clone, Copy)]
enum DatasetInstances<'a> {
    /// As the entity holds them.
    Held(&'a [Attribute]),
    /// As a notification reports them.
    Notified(&'a [NotifiedInstance<'a>]),
}

impl<'a> DatasetInstances<'a> {
    /// Each instance with what is reported of it, in order.
    fn sources(self) -> impl Iterator<Item = (InstanceSource<'a>, InstanceReport<'a>)> {
        let (held, notified) = match self {
            DatasetInstances::Held(instances) => (instances, &[][..]),
            DatasetInstances::Notified(instances) => (&[][..], instances),
        };
        held.iter()
            .map(|attribute| (InstanceSource::Typed(attribute), InstanceReport::default()))
            .chain(notified.iter().map(|instance| (instance.source, instance.report)))
    }
}

/// Write the clause 4.5.4 `dataset` map of a multi-attribute: one member per
/// `datasetId`, `@none` for the default instance.
fn dataset_map<S: Serializer>(serializer: S, instances: DatasetInstances<'_>, member: &SimplifiedMember<'_>) -> Result<S::Ok, S::Error> {
    let mut wrapper = serializer.serialize_map(Some(1))?;
    wrapper.serialize_entry(compact_properties::DATASET, &DatasetMap { instances, member })?;
    wrapper.end()
}

/// The `dataset` map of a multi-attribute's instances. A key written twice
/// keeps the instance written last, as a JSON object built member by member
/// does.
struct DatasetMap<'m, 'a> {
    /// The instances, in the order they are held or reported.
    instances: DatasetInstances<'a>,
    /// The member they belong to.
    member: &'m SimplifiedMember<'a>,
}

impl Serialize for DatasetMap<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        for (index, (source, report)) in self.instances.sources().enumerate() {
            let key = dataset_key(source.dataset_id());
            let superseded = self
                .instances
                .sources()
                .skip(index + 1)
                .any(|(later, _)| dataset_key(later.dataset_id()) == key);
            if !superseded {
                map.serialize_entry(key, &self.member.instance(source, report))?;
            }
        }
        map.end()
    }
}

/// The `dataset` map key of an instance: its `datasetId`, or `@none`.
fn dataset_key(dataset_id: Option<&str>) -> &str {
    dataset_id.unwrap_or(DEFAULT_INSTANCE_TOKEN)
}

/// One attribute instance in the simplified representation.
struct SimplifiedInstance<'a> {
    /// The instance written.
    source: InstanceSource<'a>,
    /// What a notification reports of it; the simplified representation
    /// keeps only the Null of a reported deletion.
    report: InstanceReport<'a>,
    /// What the walk applies on the way.
    walk: MemberWalk<'a>,
    /// How an instance carried verbatim is read.
    verbatim: VerbatimReading,
}

impl Serialize for SimplifiedInstance<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.source {
            InstanceSource::Typed(attribute) => match self.report.null {
                Some(null) => simplified_null(serializer, attribute.attribute_type(), null),
                None => SimplifiedTyped { attribute, walk: self.walk }.serialize(serializer),
            },
            InstanceSource::Verbatim(value) => {
                // A simplified member keeps nothing that tells a target from a
                // value, so its targets are decoded before it is simplified.
                let reported = reported_verbatim(value, &self.report);
                let decoded = decoded_attribute(&reported, self.walk.decoding);
                match self.verbatim {
                    VerbatimReading::Envelope => simplify_raw_envelope(&decoded).serialize(serializer),
                    // The document reading takes the member owned; one carried
                    // verbatim is small and rare.
                    VerbatimReading::Document => simplify_attr_value(decoded.into_owned()).serialize(serializer),
                }
            }
        }
    }
}

/// The simplified value of a deleted instance of `family`: the Null its value
/// member takes, in the one member clause 4.5.4 keeps for the families that
/// keep one.
fn simplified_null<S: Serializer>(serializer: S, family: AttributeType, null: NullValue) -> Result<S::Ok, S::Error> {
    match family {
        AttributeType::LanguageProperty | AttributeType::VocabProperty | AttributeType::JsonProperty => wrapped(serializer, family, &null),
        AttributeType::Property | AttributeType::GeoProperty | AttributeType::Relationship | AttributeType::ListProperty | AttributeType::ListRelationship => {
            null.serialize(serializer)
        }
    }
}

/// One typed attribute instance in the simplified representation.
struct SimplifiedTyped<'a> {
    /// The instance written.
    attribute: &'a Attribute,
    /// What the walk applies on the way.
    walk: MemberWalk<'a>,
}

impl Serialize for SimplifiedTyped<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let decoding = self.walk.decoding;
        match self.attribute {
            Attribute::LanguageProperty(language_property) => match self.walk.language_filter.and_then(|filter| negotiated_entry(language_property, filter)) {
                Some(entry) => entry.value.serialize(serializer),
                None => wrapped(serializer, AttributeType::LanguageProperty, &language_property.language_map),
            },
            Attribute::Property(property) => ValueBody(&property.value).serialize(serializer),
            Attribute::GeoProperty(geo_property) => GeometryBody(geo_property.value.form()).serialize(serializer),
            Attribute::Relationship(relationship) => match relationship_entities(relationship, ResponseFormat::Simplified, self.walk) {
                Some(linked) => linked.serialize(serializer),
                None => DecodedTargets::new(&relationship.object, decoding).serialize(serializer),
            },
            Attribute::VocabProperty(vocab_property) => wrapped(serializer, AttributeType::VocabProperty, &vocab_property.vocab),
            Attribute::JsonProperty(json_property) => wrapped(serializer, AttributeType::JsonProperty, &json_property.json),
            Attribute::ListProperty(list_property) => ValueListBody(&list_property.value_list).serialize(serializer),
            Attribute::ListRelationship(list_relationship) => match list_relationship_entities(list_relationship, ResponseFormat::Simplified, self.walk) {
                Some(linked) => linked.serialize(serializer),
                // Clause 4.5.4: "an ordered array holding the
                // `ListRelationship`'s Objects (represented as URIs)", the
                // concise list.
                None => ObjectListBody::new(&list_relationship.object_list, Reification::Concise, decoding).serialize(serializer),
            },
        }
    }
}

/// A value wrapped in the one member clause 4.5.4 keeps for its family.
fn wrapped<S: Serializer, T: Serialize>(serializer: S, family: AttributeType, value: &T) -> Result<S::Ok, S::Error> {
    let mut wrapper = serializer.serialize_map(Some(1))?;
    wrapper.serialize_entry(family.compact_value_key(), value)?;
    wrapper.end()
}
