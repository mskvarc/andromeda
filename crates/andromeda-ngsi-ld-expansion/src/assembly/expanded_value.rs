//! Reading an expanded item as the model's typed expanded value.
//!
//! A Property value, a list item, a member of a structured value and a
//! data-integrity seal are all held as
//! [`andromeda_ngsi_ld_entity::expanded_value::ExpandedValue`]. A written
//! fragment may delete a member of a structured value with the NGSI-LD Null
//! (clause 5.5.12), which only the fragment mode can hold; state never can.

use crate::{
    assembly::{geometry_node::geometry_value_node, null_sentinel::is_sole_null},
    expanded_tree::{ExpandedItem, ExpandedNode, Scalar, ValueObject},
};
use andromeda_ngsi_ld_entity::{
    expanded_value::{ExpandedValue, ValueNode},
    json_literal::JsonLiteral,
    literal::{Literal, LiteralValue},
    member_shape::MemberShape,
    node_id::NodeId,
    refusal::EntityRefusal,
    value_mode::{Fragment, FragmentMember, Stored, ValueMode},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, reference::NgsiLdReference};
use andromeda_ngsi_ld_number::JsonNumber;
use compact_str::CompactString;
use indexmap::IndexMap;
use langtag::LangTagBuf;

/// How one member of a structured value is held in a value mode.
pub trait MemberMode: ValueMode {
    /// The member holding `values`, or the deletion of the member when the
    /// written values were the NGSI-LD Null alone.
    fn member(values: Vec<ExpandedValue<Self>>, deletion: bool) -> Self::Member;
}

impl MemberMode for Stored {
    fn member(values: Vec<ExpandedValue<Stored>>, _deletion: bool) -> Vec<ExpandedValue<Stored>> {
        values
    }
}

impl MemberMode for Fragment {
    fn member(values: Vec<ExpandedValue<Fragment>>, deletion: bool) -> FragmentMember {
        if deletion { FragmentMember::Remove } else { FragmentMember::Values(values) }
    }
}

/// Read every item of a member as an expanded value.
///
/// # Errors
///
/// As [`expanded_value`].
pub fn expanded_values<M: MemberMode>(attribute: &AttributeName, items: Vec<ExpandedItem>) -> Result<Vec<ExpandedValue<M>>, EntityRefusal> {
    items.into_iter().map(|item| expanded_value(attribute, item)).collect()
}

/// Read one item as an expanded value.
///
/// # Errors
///
/// Returns [`EntityRefusal::Malformed`] for a number out of the supported
/// range, a datatype, language or node identifier that is not one, and
/// [`EntityRefusal::ExpandedAttributeName`] for a member name that is no IRI.
pub fn expanded_value<M: MemberMode>(attribute: &AttributeName, item: ExpandedItem) -> Result<ExpandedValue<M>, EntityRefusal> {
    match item {
        ExpandedItem::Value(value) => literal(attribute, value).map(ExpandedValue::Literal),
        ExpandedItem::Json(json) => Ok(ExpandedValue::Json(JsonLiteral::new(json))),
        ExpandedItem::Node(node) => value_node(attribute, node).map(ExpandedValue::Node),
        ExpandedItem::List(items) => expanded_values(attribute, items).map(ExpandedValue::List),
        ExpandedItem::Geometry(form) => geometry_value_node(attribute, &form),
    }
}

/// Read a value object as a literal.
///
/// # Errors
///
/// Returns [`EntityRefusal::Malformed`] for a part that is not what it names.
pub fn literal(attribute: &AttributeName, value: ValueObject) -> Result<Literal, EntityRefusal> {
    let malformed = || nested_malformed(attribute);
    let scalar = match value.scalar {
        Scalar::Number(text) => LiteralValue::Number(text.parse::<JsonNumber>().map_err(|_| malformed())?),
        Scalar::String(text) => LiteralValue::String(CompactString::from(text)),
        Scalar::Boolean(flag) => LiteralValue::Boolean(flag),
    };
    let datatype = value
        .datatype
        .map(|datatype| datatype.parse::<NgsiLdReference>())
        .transpose()
        .map_err(|_| malformed())?;
    let language = value.language.map(LangTagBuf::new).transpose().map_err(|_| malformed())?;
    Literal::try_new(scalar, datatype, language, value.direction).map_err(|_| malformed())
}

/// Read a node object of a structured value.
fn value_node<M: MemberMode>(attribute: &AttributeName, node: ExpandedNode) -> Result<ValueNode<M>, EntityRefusal> {
    let id = node.id.map(|id| id.parse::<NodeId>()).transpose().map_err(|_| nested_malformed(attribute))?;
    let types = node
        .types
        .into_iter()
        .map(|type_name| type_name.parse::<NgsiLdReference>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| nested_malformed(attribute))?;
    let mut members = IndexMap::with_capacity(node.members.len());
    for (name, items) in node.members {
        let name = name.parse::<AttributeName>().map_err(EntityRefusal::ExpandedAttributeName)?;
        let deletion = is_sole_null(&items);
        members.insert(name, M::member(expanded_values(attribute, items)?, deletion));
    }
    Ok(ValueNode::new(id, types, members))
}

/// The refusal of a nested value that is not what its expanded form names.
fn nested_malformed(attribute: &AttributeName) -> EntityRefusal {
    EntityRefusal::Malformed {
        attribute: Some(attribute.clone()), // clone: the refusal names the attribute the caller keeps
        member: MemberShape::NestedValue,
    }
}
