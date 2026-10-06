//! One entity in the aggregated representation of a Temporal Evolution (ETSI
//! GS CIM 009 clause 4.5.19, and the grouping extension) written straight from
//! its typed form.
//!
//! Each attribute is one object per `datasetId`, naming the family's `type`
//! and the `datasetId` when there is one, and holding, for every requested
//! method, its `[value, start, end]` triples, nested one map key per grouping
//! term under the grouped extension. The clause 4.3.6.8 fallback reaches the
//! family and the `datasetId`; the aggregation vocabulary appears in no row of
//! the fallback tables, so it is written as it is.

use crate::{
    contextual_body::open_object,
    entity_body::{instance_fallback::kept_instance, member_walk::MemberWalk},
    version::fallback::InstanceMember,
};
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_model::temporal::aggregated_entity::{AggregatedAttribute, AggregatedTemporalEntity, MethodAggregate};
use serde::{Serialize, Serializer, ser::SerializeMap};

/// One entity in the aggregated representation.
#[derive(Debug, Clone, Copy)]
pub(crate) struct AggregatedBody<'a> {
    /// The aggregated entity.
    pub(crate) entity: &'a AggregatedTemporalEntity,
    /// What the walk applies on the way.
    pub(crate) walk: MemberWalk<'a>,
    /// The `@context` member written beside the entity's own, if the body
    /// carries one.
    pub(crate) context: Option<&'a JsonLiteral>,
}

impl Serialize for AggregatedBody<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let entity = self.entity;
        let mut object = open_object(serializer, self.context, None)?;
        object.serialize_entry("id", &entity.id)?;
        match entity.type_.iter().next() {
            Some(first) if self.walk.writes_single_type() => object.serialize_entry("type", first)?,
            Some(_) | None => object.serialize_entry("type", &entity.type_)?,
        }
        for (name, attributes) in &entity.attrs {
            object.serialize_entry(name.as_str(), &AggregatedMember { attributes, walk: self.walk })?;
        }
        object.end()
    }
}

/// The aggregated instances of one attribute.
struct AggregatedMember<'a> {
    /// The instances, one per `datasetId`.
    attributes: &'a [AggregatedAttribute],
    /// What the walk applies on the way.
    walk: MemberWalk<'a>,
}

impl Serialize for AggregatedMember<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Notes 2 and 3 of Table 4.3.6.8-1: the outer array is the set of
        // instances separated by `datasetId`, which exists from 1.3 onwards.
        if self.walk.collapses_instances()
            && let Some(kept) = kept_instance(self.attributes, |attribute| attribute.dataset_id.as_ref())
        {
            return self.instance(kept).serialize(serializer);
        }
        // Clause 4.5.4: one instance is an object and several an array.
        match self.attributes {
            [only] => self.instance(only).serialize(serializer),
            several => serializer.collect_seq(several.iter().map(|attribute| self.instance(attribute))),
        }
    }
}

impl<'a> AggregatedMember<'a> {
    /// One instance's object.
    const fn instance(&self, attribute: &'a AggregatedAttribute) -> AggregatedInstance<'a> {
        AggregatedInstance { attribute, walk: self.walk }
    }
}

/// One aggregated attribute instance.
struct AggregatedInstance<'a> {
    /// The instance.
    attribute: &'a AggregatedAttribute,
    /// What the walk applies on the way.
    walk: MemberWalk<'a>,
}

impl Serialize for AggregatedInstance<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let attribute = self.attribute;
        let family = self.walk.written_family(attribute.family);
        let mut object = serializer.serialize_map(None)?;
        object.serialize_entry("type", &family)?;
        if let Some(dataset_id) = &attribute.dataset_id
            && self.walk.keeps_instance_member(family, InstanceMember::DatasetId)
        {
            object.serialize_entry("datasetId", dataset_id)?;
        }
        for MethodAggregate { method, values } in &attribute.aggregates {
            object.serialize_entry(<&'static str>::from(*method), values)?;
        }
        object.end()
    }
}
