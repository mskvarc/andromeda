//! One attribute member of a merge fragment (ETSI GS CIM 009 clause 5.5.12),
//! read from a compact node as clause 4.5.2.3 and its siblings read any
//! attribute member, with the instances a merge writes.
//!
//! Every level of nesting runs on sufficient stack, as the reading of an
//! entity's members does.

use crate::{
    attribute::{member_node::array_refusal, member_reading::MemberReading, member_value_reading::ReadError, merge_instance::MergeInstance, structure_budget},
    node_refusal::NodeRefusal,
};
use andromeda_native_stack::on_sufficient_stack;
use andromeda_ngsi_ld_representation::{
    attribute_type_validation::AttributeNodeFault,
    compact_node::{CompactNode, NodeShape},
    concise_reading::{ArrayReading, ObjectReading, read_array, read_object},
};
use andromeda_ngsi_ld_vocabulary::ngsi_ld_null::NULL_URI;
use serde::{Serialize, Serializer};
use std::slice::{from_mut, from_ref};

/// One attribute member of a merge fragment.
#[derive(Debug, Clone, PartialEq)]
pub enum MergeMember {
    /// The NGSI-LD Null in place of the whole member: the attribute, or the
    /// sub-attribute, is removed (clause 5.5.12).
    NgsiLdNull,
    /// One instance.
    Single(Box<MergeInstance>),
    /// Several instances, each addressed by its `datasetId` (clause 4.5.5).
    Multiple(Vec<MergeInstance>),
}

impl MergeMember {
    /// The instances the member writes, each open to rewriting.
    pub fn instances_mut(&mut self) -> impl Iterator<Item = &mut MergeInstance> {
        let instances: &mut [MergeInstance] = match self {
            MergeMember::NgsiLdNull => &mut [],
            MergeMember::Single(instance) => from_mut(instance.as_mut()),
            MergeMember::Multiple(instances) => instances,
        };
        instances.iter_mut()
    }

    /// The instances the member writes; the NGSI-LD Null writes none.
    pub fn instances(&self) -> impl Iterator<Item = &MergeInstance> {
        let instances: &[MergeInstance] = match self {
            MergeMember::NgsiLdNull => &[],
            MergeMember::Single(instance) => from_ref(instance.as_ref()),
            MergeMember::Multiple(instances) => instances,
        };
        instances.iter()
    }
}

impl MemberReading for MergeMember {
    fn from_node<'n, N: CompactNode<'n>>(name: &str, member: N) -> Result<MergeMember, ReadError<'n, N>> {
        on_sufficient_stack(|| member_value(name, member))
    }

    fn from_fragment_node<'n, N: CompactNode<'n>>(name: &str, fragment: N) -> Result<MergeMember, ReadError<'n, N>> {
        on_sufficient_stack(|| {
            let NodeShape::Object(members) = fragment.shape() else {
                return member_value(name, fragment);
            };
            match read_object(&members).map_err(|refusal| ReadError::<N>::from(NodeRefusal::Shape(AttributeNodeFault::of_object(refusal))))? {
                ObjectReading::Instance(family) => MergeInstance::from_members(name, Some(family), fragment, members).map(single),
                ObjectReading::GeoPropertyValue => MergeInstance::concise_geo_property(fragment).map(single),
                // The fragment is the instance it updates, whatever it names.
                ObjectReading::PropertyValue => MergeInstance::from_members(name, None, fragment, members).map(single),
            }
        })
    }
}

// Hand-written: the NGSI-LD Null is written as its string, beside the
// instances, which no derive of an untagged enum with a unit variant writes.
impl Serialize for MergeMember {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            MergeMember::NgsiLdNull => serializer.serialize_str(NULL_URI.as_str()),
            MergeMember::Single(instance) => instance.serialize(serializer),
            MergeMember::Multiple(instances) => instances.serialize(serializer),
        }
    }
}

/// Read one merge member on the stack the caller left.
fn member_value<'n, N: CompactNode<'n>>(name: &str, member: N) -> Result<MergeMember, ReadError<'n, N>> {
    if member.is_member_ngsi_ld_null() {
        return Ok(MergeMember::NgsiLdNull);
    }
    match member.shape() {
        NodeShape::Array(items) => match read_array(&items).map_err(array_refusal::<N>)? {
            ArrayReading::Instances => items
                .into_iter()
                .enumerate()
                .take_while(|(_, item)| !structure_budget::is_spent(*item))
                .map(|(index, item)| {
                    structure_budget::spend(item);
                    instance(name, item).map_err(|error| error.within_element(index))
                })
                .collect::<Result<Vec<_>, _>>()
                .map(MergeMember::Multiple),
            ArrayReading::PropertyValue => MergeInstance::concise_property(member).map(single),
        },
        NodeShape::Null | NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::String(_) | NodeShape::Object(_) | NodeShape::Geometry(_) => {
            instance(name, member).map(single)
        }
    }
}

/// Read one node written where one instance stands.
fn instance<'n, N: CompactNode<'n>>(name: &str, node: N) -> Result<MergeInstance, ReadError<'n, N>> {
    match node.shape() {
        NodeShape::Null => Err(NodeRefusal::Null.into()),
        NodeShape::Geometry(_) => MergeInstance::concise_geo_property(node),
        NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::String(_) | NodeShape::Array(_) => MergeInstance::concise_property(node),
        NodeShape::Object(members) => {
            match read_object(&members).map_err(|refusal| ReadError::<N>::from(NodeRefusal::Shape(AttributeNodeFault::of_object(refusal))))? {
                ObjectReading::Instance(family) => MergeInstance::from_members(name, Some(family), node, members),
                ObjectReading::GeoPropertyValue => MergeInstance::concise_geo_property(node),
                ObjectReading::PropertyValue => MergeInstance::concise_property(node),
            }
        }
    }
}

/// One instance as the member it is alone.
fn single(instance: MergeInstance) -> MergeMember {
    MergeMember::Single(Box::new(instance))
}
