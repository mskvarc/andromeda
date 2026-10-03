//! Reading one attribute member of an entity from a compact node (ETSI GS CIM
//! 009 clauses 4.5.5 and 5.2.4).
//!
//! A member is one attribute instance, an array of them, or a value carried
//! through verbatim, tried in that order: an object that is no instance of the
//! family it names, and an array holding anything but instances, are carried
//! as written. This is the reading a decoded document's members get
//! ([`AttributeValue::from_member`]) and the one a compacted entity's members
//! get, so the two cannot disagree on which members are typed.
//!
//! An instance's sub-attributes are members too (clause 4.5.2), read by the
//! reading of the instance, so the reading recurses once per level of nesting,
//! about 40 KiB of stack per level in an unoptimized build. Every level runs on
//! sufficient stack.

use crate::{
    attribute::instance::{Attribute, AttributeValue},
    compact_node::{CompactNode, NodeShape},
    node_refusal::NodeReadError,
};
use andromeda_native_stack::on_sufficient_stack;

/// What an attribute member reads as.
#[derive(Debug)]
pub(crate) enum MemberForm {
    /// One attribute instance.
    Single(Box<Attribute>),
    /// Several instances of the attribute.
    Multiple(Vec<Attribute>),
    /// Neither: the member is carried as written.
    Verbatim,
}

/// Read one attribute member.
///
/// # Errors
///
/// Returns the node's [`CompactNode::Unreadable`] when a node of the member
/// has no JSON form.
pub(crate) fn read_member<N: CompactNode>(member: &N) -> Result<MemberForm, N::Unreadable> {
    on_sufficient_stack(|| member_form(member))
}

/// Read one attribute member on the stack the caller left.
fn member_form<N: CompactNode>(member: &N) -> Result<MemberForm, N::Unreadable> {
    match member.shape() {
        NodeShape::Object(_) => Ok(match instance(member)? {
            Some(instance) => MemberForm::Single(Box::new(instance)),
            None => MemberForm::Verbatim,
        }),
        NodeShape::Array(items) => {
            let mut instances = Vec::with_capacity(items.len());
            for item in items {
                let Some(instance) = instance(item)? else {
                    return Ok(MemberForm::Verbatim);
                };
                instances.push(instance);
            }
            Ok(MemberForm::Multiple(instances))
        }
        // A geometry names its `GeoJSON` type, never an attribute family.
        NodeShape::Null | NodeShape::Boolean | NodeShape::Number | NodeShape::String(_) | NodeShape::Geometry(_) => Ok(MemberForm::Verbatim),
    }
}

/// The instance `node` is, or `None` when it is refused as one.
fn instance<N: CompactNode>(node: &N) -> Result<Option<Attribute>, N::Unreadable> {
    match Attribute::from_node(node) {
        Ok(instance) => Ok(Some(instance)),
        Err(NodeReadError::Refused(_)) => Ok(None),
        Err(NodeReadError::Unreadable(unreadable)) => Err(unreadable),
    }
}

impl AttributeValue {
    /// The attribute member a compact node is: an instance, an array of
    /// instances, or, when it is neither, the JSON value the node stands for.
    ///
    /// # Errors
    ///
    /// Returns the node's [`CompactNode::Unreadable`] when a node of the
    /// member has no JSON form.
    pub fn from_node<N: CompactNode>(member: &N) -> Result<AttributeValue, N::Unreadable> {
        Ok(match read_member(member)? {
            MemberForm::Single(instance) => AttributeValue::Single(instance),
            MemberForm::Multiple(instances) => AttributeValue::Multiple(instances),
            MemberForm::Verbatim => AttributeValue::Raw(member.to_json()?),
        })
    }
}
