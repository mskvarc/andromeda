//! Reading one attribute member of an entity from a compact node (ETSI GS CIM
//! 009 clauses 4.5.2.3, 4.5.5 and 5.2.4).
//!
//! A node written where an attribute stands is read as clause 4.5.2.3 and its
//! siblings read it ([`andromeda_ngsi_ld_representation::concise_reading`]):
//! an object is an instance of the family its `type` names or its value
//! member implies, a bare geometry a concise `GeoProperty`, anything else the
//! value of a concise Property; an array is the instances of a multi-attribute
//! or one value; the NGSI-LD Null in place of the whole member, in any spelling
//! that expands to it, is that Null. A node no clause admits is refused where
//! it stands. This is the reading every
//! compact document gets -- a request, a peer's answer, a notification, a
//! compacted entity -- so they cannot disagree on what a member is.
//!
//! An instance's sub-attributes are members too (clause 4.5.2), read by the
//! reading of the instance, so the reading recurses once per level of nesting,
//! about 40 KiB of stack per level in an unoptimized build. Every level runs on
//! sufficient stack.

use crate::{
    attribute::{
        instance::{Attribute, AttributeValue},
        instance_member::ReadError,
        member_reading::MemberReading,
        structure_budget,
    },
    node_refusal::{NodeRefusal, RefusedMember},
};
use andromeda_native_stack::on_sufficient_stack;
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use andromeda_ngsi_ld_representation::{
    attribute_type_validation::AttributeNodeFault,
    compact_node::{CompactNode, NodeShape},
    concise_reading::{ArrayReading, ArrayRefusal, ObjectReading, read_array, read_object},
};

impl MemberReading for AttributeValue {
    fn from_node<'n, N: CompactNode<'n>>(name: &str, member: N) -> Result<AttributeValue, ReadError<'n, N>> {
        on_sufficient_stack(|| member_value(name, member))
    }

    fn from_fragment_node<'n, N: CompactNode<'n>>(name: &str, fragment: N) -> Result<AttributeValue, ReadError<'n, N>> {
        on_sufficient_stack(|| {
            let NodeShape::Object(members) = fragment.shape() else {
                return member_value(name, fragment);
            };
            match read_object(&members).map_err(|refusal| ReadError::<N>::from(NodeRefusal::Shape(AttributeNodeFault::of_object(refusal))))? {
                ObjectReading::Instance(family) => Attribute::from_members(name, family, members).map(single),
                ObjectReading::GeoPropertyValue => Attribute::concise_geo_property(fragment).map(single),
                // The fragment is the instance itself, and a whole attribute
                // requires the value its family holds (clause 5.2.5).
                ObjectReading::PropertyValue => Err(NodeRefusal::Missing {
                    member: RefusedMember::Value(AttributeType::Property),
                }
                .into()),
            }
        })
    }
}

impl Attribute {
    /// The attribute instance a compact node written under the attribute
    /// name `name`, where one instance stands, is.
    ///
    /// # Errors
    ///
    /// As [`MemberReading::from_node`].
    pub fn from_node<'n, N: CompactNode<'n>>(name: &str, node: N) -> Result<Attribute, ReadError<'n, N>> {
        on_sufficient_stack(|| instance(name, node))
    }
}

/// Read one attribute member on the stack the caller left.
fn member_value<'n, N: CompactNode<'n>>(name: &str, member: N) -> Result<AttributeValue, ReadError<'n, N>> {
    if member.is_member_ngsi_ld_null() {
        return Ok(AttributeValue::NgsiLdNull);
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
                .map(AttributeValue::Multiple),
            ArrayReading::PropertyValue => Attribute::concise_property(member).map(single),
        },
        NodeShape::Null | NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::String(_) | NodeShape::Object(_) | NodeShape::Geometry(_) => {
            instance(name, member).map(single)
        }
    }
}

/// Read one node written where one attribute instance stands.
fn instance<'n, N: CompactNode<'n>>(name: &str, node: N) -> Result<Attribute, ReadError<'n, N>> {
    match node.shape() {
        NodeShape::Null => Err(NodeRefusal::Null.into()),
        NodeShape::Geometry(_) => Attribute::concise_geo_property(node),
        NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::String(_) | NodeShape::Array(_) => Attribute::concise_property(node),
        NodeShape::Object(members) => {
            match read_object(&members).map_err(|refusal| ReadError::<N>::from(NodeRefusal::Shape(AttributeNodeFault::of_object(refusal))))? {
                ObjectReading::Instance(family) => Attribute::from_members(name, family, members),
                ObjectReading::GeoPropertyValue => Attribute::concise_geo_property(node),
                ObjectReading::PropertyValue => Attribute::concise_property(node),
            }
        }
    }
}

/// One instance as the member it is alone.
fn single(instance: Attribute) -> AttributeValue {
    AttributeValue::Single(Box::new(instance))
}

/// The refusal of an array no clause admits where an attribute stands,
/// located at the element at fault when one is.
pub(crate) fn array_refusal<'n, N: CompactNode<'n>>(refusal: ArrayRefusal) -> ReadError<'n, N> {
    match refusal {
        ArrayRefusal::Element { index, refusal } => ReadError::<N>::from(NodeRefusal::Shape(AttributeNodeFault::of_object(refusal))).within_element(index),
        ArrayRefusal::MixedInstancesAndValues => NodeRefusal::Shape(AttributeNodeFault::MixedInstancesAndValues).into(),
    }
}
