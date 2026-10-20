//! One attribute of a Temporal Evolution in the simplified temporal
//! representation (ETSI GS CIM 009 clause 4.5.9).
//!
//! The attribute is one object per `datasetId`, holding the family's `type`,
//! the `datasetId` when there is one, and a single plural member -- `values`,
//! `objects`, `languageMaps`, `jsons`, `vocabs`, `valueLists` or `objectLists`
//! -- whose value is the array of `[value, instant]` pairs, the instant being
//! the one on the Temporal Property the query ran on (clause 4.11, `observedAt`
//! by default). A deletion carries no instant on an observation axis, so its
//! `deletedAt` stands in for it.
//!
//! The clause 4.3.6.8 fallback applies here as it does to an instance: a family
//! the target never defined is written as the family it specializes, which
//! renames the plural member and rewrites each pair's value the way clause 4.5.9
//! encodes it there, `datasetId` is left out below 1.3 where the set of
//! datasets collapses to its default (Table 4.3.6.8-1 notes 2 and 3), and a
//! `ListRelationship` reformatted as a Relationship keeps its first target.

use crate::{
    entity_body::{family_value::FamilyValue, instance_fallback::kept_instance, member_walk::MemberWalk, reified_member::Reification},
    version::fallback::InstanceMember,
};
use andromeda_ngsi_ld_lexicon::{
    attribute_type::{AttributeType, TemporalValueEncoding},
    identifier::Identifier,
    rfc3339::sub_second,
    time_prop::TimeProp,
};
use andromeda_ngsi_ld_model::{
    attribute::{common_fields::CommonAttributeFields, instance::Attribute},
    temporal::temporal_attribute::TemporalAttribute,
};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::{
    Serialize,
    Serializer,
    ser::{SerializeMap, SerializeSeq},
};

/// One attribute of a temporal entity in the simplified representation.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ValuesMember<'a> {
    /// The attribute.
    attribute: &'a TemporalAttribute,
    /// The Temporal Property the query ran on.
    timeproperty: TimeProp,
    /// What the walk applies on the way.
    walk: MemberWalk<'a>,
}

impl<'a> ValuesMember<'a> {
    /// The attribute, written as pairs on `timeproperty` under `walk`.
    pub(crate) const fn new(attribute: &'a TemporalAttribute, timeproperty: TimeProp, walk: MemberWalk<'a>) -> ValuesMember<'a> {
        ValuesMember { attribute, timeproperty, walk }
    }

    /// The instances of the attribute, gathered by `datasetId` in the order
    /// the first of each appears. An attribute with no instance is one default
    /// dataset with none, so it is still written, with an empty member of its
    /// family, as clause 4.5.9 has it ("as many array elements as Property
    /// instances ... being represented").
    fn datasets(&self) -> Vec<Dataset<'a>> {
        let mut groups: IndexMap<Option<&'a Identifier>, Vec<&'a Attribute>> = IndexMap::new();
        for instance in self.attribute.instances() {
            groups.entry(instance.dataset_id()).or_default().push(instance);
        }
        if groups.is_empty() {
            return vec![Dataset {
                identifier: None,
                family: self.attribute.family(),
                instances: Vec::new(),
            }];
        }
        groups
            .into_iter()
            .map(|(identifier, instances)| Dataset {
                identifier,
                family: instances.first().map_or(self.attribute.family(), |first| first.attribute_type()),
                instances,
            })
            .collect()
    }
}

/// The instances of one `datasetId` of an attribute.
struct Dataset<'a> {
    /// The dataset, absent for the default instance.
    identifier: Option<&'a Identifier>,
    /// The family of its instances.
    family: AttributeType,
    /// The instances, in order.
    instances: Vec<&'a Attribute>,
}

impl Serialize for ValuesMember<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let datasets = self.datasets();
        // Notes 2 and 3 of Table 4.3.6.8-1: instances separated by `datasetId`
        // exist from 1.3 onwards, and here the outer array is exactly that set.
        if self.walk.collapses_instances()
            && let Some(kept) = kept_instance(&datasets, |dataset| dataset.identifier)
        {
            return self.dataset(kept).serialize(serializer);
        }
        match datasets.as_slice() {
            [only] => self.dataset(only).serialize(serializer),
            several => serializer.collect_seq(several.iter().map(|dataset| self.dataset(dataset))),
        }
    }
}

impl<'a> ValuesMember<'a> {
    /// One dataset's object.
    const fn dataset<'d>(&self, dataset: &'d Dataset<'a>) -> DatasetObject<'d, 'a> {
        DatasetObject {
            dataset,
            written_family: self.walk.written_family(dataset.family),
            timeproperty: self.timeproperty,
            walk: self.walk,
        }
    }
}

/// One dataset's object of the simplified representation.
struct DatasetObject<'d, 'a> {
    /// The dataset.
    dataset: &'d Dataset<'a>,
    /// The family the dataset is written as.
    written_family: AttributeType,
    /// The Temporal Property the query ran on.
    timeproperty: TimeProp,
    /// What the walk applies on the way.
    walk: MemberWalk<'a>,
}

impl Serialize for DatasetObject<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut object = serializer.serialize_map(None)?;
        object.serialize_entry("type", &self.written_family)?;
        if let Some(dataset_id) = self.dataset.identifier
            && self.walk.keeps_instance_member(self.written_family, InstanceMember::DatasetId)
        {
            object.serialize_entry("datasetId", dataset_id)?;
        }
        object.serialize_entry(self.written_family.temporal_values_key(), &Pairs { dataset: self })?;
        object.end()
    }
}

/// The `[value, instant]` pairs of one dataset.
struct Pairs<'p, 'd, 'a> {
    /// The dataset whose pairs are written.
    dataset: &'p DatasetObject<'d, 'a>,
}

impl Serialize for Pairs<'_, '_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let object = self.dataset;
        serializer.collect_seq(object.dataset.instances.iter().map(|instance| Pair {
            instance,
            written_family: object.written_family,
            timeproperty: object.timeproperty,
        }))
    }
}

/// One `[value, instant]` pair.
struct Pair<'a> {
    /// The instance the pair stands for.
    instance: &'a Attribute,
    /// The family the pair is written as.
    written_family: AttributeType,
    /// The Temporal Property the query ran on.
    timeproperty: TimeProp,
}

impl Serialize for Pair<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut pair = serializer.serialize_seq(Some(2))?;
        pair.serialize_element(&PairValue {
            instance: self.instance,
            written_family: self.written_family,
        })?;
        pair.serialize_element(&instant(self.instance.common(), self.timeproperty).map(Instant))?;
        pair.end()
    }
}

/// The value half of one pair, read out of the instance's own member.
///
/// Three families wrap their value in a one-member object (clause 4.5.9's
/// `languageMaps`, `jsons` and `vocabs`); the rest carry it bare. A
/// `ListRelationship` is the one family whose bare value is not the member it
/// was read from: clause 4.5.9 gives its `objectLists` pairs "an ordered array
/// of Relationship objects (URIs)" -- EXAMPLE 7 spells them as bare strings --
/// while its normalized `objectList` holds the `{"object": ...}` members clause
/// 4.5.22.2 mandates, so it is written in the concise form that holds bare URIs.
struct PairValue<'a> {
    /// The instance.
    instance: &'a Attribute,
    /// The family the pair is written as.
    written_family: AttributeType,
}

impl Serialize for PairValue<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let value = FamilyValue::current(self.instance, Reification::Concise).written_as(self.written_family);
        match self.written_family.temporal_value_encoding() {
            TemporalValueEncoding::Wrapped => {
                let mut wrapper = serializer.serialize_map(Some(1))?;
                wrapper.serialize_entry(self.written_family.compact_value_key(), &value)?;
                wrapper.end()
            }
            TemporalValueEncoding::Bare => value.serialize(serializer),
        }
    }
}

/// The instant a pair carries: the one on the Temporal Property the query ran
/// on, or a deletion's `deletedAt`.
fn instant(common: &CommonAttributeFields, timeproperty: TimeProp) -> Option<DateTime<Utc>> {
    match timeproperty {
        TimeProp::Observed => common.observed_at,
        TimeProp::Created => common.created_at,
        TimeProp::Modified => common.modified_at,
        TimeProp::Deleted => common.deleted_at,
    }
    .or(common.deleted_at)
}

/// An instant spelled with exactly the sub-second precision it carries.
struct Instant(DateTime<Utc>);

impl Serialize for Instant {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&sub_second(self.0))
    }
}
