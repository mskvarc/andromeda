//! Reading an expanded item as the model's typed expanded value.
//!
//! A Property value, a list item, a member of a structured value and a
//! data-integrity seal are all held as
//! [`andromeda_ngsi_ld_entity::expanded_value::ExpandedValue`]. A written
//! fragment may delete a member of a structured value with the NGSI-LD Null
//! (clause 5.5.12), which only the fragment mode can hold; state never can.

use crate::{
    assembly::{
        geometry_node::geometry_value_node,
        member_value::null_beside_data,
        null_sentinel::{is_null, is_sole_null},
    },
    expanded_tree::{ExpandedItem, ExpandedNode, Scalar, ValueObject},
};
use andromeda_ngsi_ld_entity::{
    expanded_value::{ExpandedValue, ValueNode},
    expanded_value_name::ExpandedValueName,
    literal::{Literal, LiteralValue},
    member_shape::MemberShape,
    node_id::NodeId,
    refusal::EntityRefusal,
    value_mode::{Fragment, FragmentMember, Stored, ValueMode},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, reference::NgsiLdReference};
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_ngsi_ld_vocabulary::ngsi_ld_null::is_ngsi_ld_null;
use compact_str::CompactString;
use indexmap::IndexMap;
use langtag::LangTagBuf;

/// How one member of a structured value is held in a value mode.
pub trait MemberMode: ValueMode {
    /// The member holding `values`.
    fn values(values: Vec<ExpandedValue<Self>>) -> Self::Member;

    /// The member written as the NGSI-LD Null alone, for a structured value of
    /// `attribute`.
    ///
    /// # Errors
    ///
    /// Returns the refusal of a mode that cannot hold the deletion.
    fn deletion(attribute: &AttributeName) -> Result<Self::Member, EntityRefusal>;
}

impl MemberMode for Stored {
    fn values(values: Vec<ExpandedValue<Stored>>) -> Vec<ExpandedValue<Stored>> {
        values
    }

    fn deletion(attribute: &AttributeName) -> Result<Vec<ExpandedValue<Stored>>, EntityRefusal> {
        Err(EntityRefusal::NullInState {
            attribute: Some(attribute.clone()), // clone: the refusal names the attribute the caller keeps
        })
    }
}

impl MemberMode for Fragment {
    fn values(values: Vec<ExpandedValue<Fragment>>) -> FragmentMember {
        FragmentMember::Values(values)
    }

    /// Clause 5.5.12: the merge deletes the member.
    fn deletion(_attribute: &AttributeName) -> Result<FragmentMember, EntityRefusal> {
        Ok(FragmentMember::Remove)
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
/// Returns [`EntityRefusal::NullBesideData`] for the NGSI-LD Null, which is
/// never data, [`EntityRefusal::Malformed`] for a number out of the supported
/// range or a datatype or language that is not one, and
/// [`EntityRefusal::ExpandedMemberName`] for a member name or a node type
/// expansion never keeps.
pub fn expanded_value<M: MemberMode>(attribute: &AttributeName, item: ExpandedItem) -> Result<ExpandedValue<M>, EntityRefusal> {
    if is_null(&item) {
        return Err(null_beside_data(attribute));
    }
    match item {
        ExpandedItem::Value(value) => literal(attribute, value).map(ExpandedValue::Literal),
        ExpandedItem::Json(json) => Ok(ExpandedValue::Json(json)),
        ExpandedItem::Node(node) => value_node(attribute, node).map(ExpandedValue::Node),
        ExpandedItem::List(items) => expanded_values(attribute, items).map(ExpandedValue::List),
        ExpandedItem::Geometry(geometry) => geometry_value_node(attribute, *geometry),
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
    // A node that is the Null alone was read as the Null before it came here;
    // one naming the Null beside types or members is the Null beside data.
    if node.id.as_deref().is_some_and(is_ngsi_ld_null) {
        return Err(null_beside_data(attribute));
    }
    let id = node.id.as_deref().map(NodeId::from);
    let types = node
        .types
        .into_iter()
        .map(|type_name| type_name.parse::<ExpandedValueName>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(EntityRefusal::ExpandedMemberName)?;
    let mut members = IndexMap::with_capacity(node.members.len());
    for (name, items) in node.members {
        let name = name.parse::<ExpandedValueName>().map_err(EntityRefusal::ExpandedMemberName)?;
        members.insert(name, node_member::<M>(attribute, items)?);
    }
    Ok(ValueNode::new(id, types, members))
}

/// One member of a structured value: its values, or its deletion when they
/// are the NGSI-LD Null alone (clause 5.5.12).
///
/// # Errors
///
/// As [`expanded_value`], and the refusal of a mode that holds no deletion.
pub fn node_member<M: MemberMode>(attribute: &AttributeName, items: Vec<ExpandedItem>) -> Result<M::Member, EntityRefusal> {
    if is_sole_null(&items) {
        M::deletion(attribute)
    } else {
        expanded_values(attribute, items).map(M::values)
    }
}

/// The refusal of a nested value that is not what its expanded form names.
fn nested_malformed(attribute: &AttributeName) -> EntityRefusal {
    EntityRefusal::Malformed {
        attribute: Some(attribute.clone()), // clone: the refusal names the attribute the caller keeps
        member: MemberShape::NestedValue,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        assembly::expanded_value::expanded_value,
        expanded_tree::{ExpandedItem, ExpandedNode, Scalar, ValueObject},
    };
    use andromeda_ngsi_ld_entity::{
        expanded_value::ExpandedValue,
        refusal::EntityRefusal,
        value_mode::{Fragment, FragmentMember, Stored},
    };
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;

    fn attribute() -> AttributeName {
        "https://example.org/address".parse().expect("an attribute name")
    }

    fn text(value: &str) -> ExpandedItem {
        ExpandedItem::Value(ValueObject::plain(Scalar::String(value.to_owned())))
    }

    /// A structured value whose member `street` holds `items`.
    fn structured(items: Vec<ExpandedItem>) -> ExpandedItem {
        ExpandedItem::Node(ExpandedNode {
            members: vec![("https://example.org/street".to_owned(), items)],
            ..ExpandedNode::default()
        })
    }

    /// Clause 5.5.12: a merge fragment deletes a member set to the Null alone.
    #[test]
    fn a_member_that_is_the_null_alone_is_a_deletion_in_a_fragment() {
        let value = expanded_value::<Fragment>(&attribute(), structured(vec![text("urn:ngsi-ld:null")])).expect("a fragment holds the deletion");
        let ExpandedValue::Node(node) = value else {
            panic!("a node, got {value:?}");
        };
        pretty_assertions::assert_eq!(node.members().get("https://example.org/street"), Some(&FragmentMember::Remove));
    }

    /// Entity state never holds the Null, so the stored mode refuses the deletion
    /// instead of keeping the Null as a value.
    #[test]
    fn a_member_that_is_the_null_alone_is_refused_in_state() {
        let refusal = expanded_value::<Stored>(&attribute(), structured(vec![text("urn:ngsi-ld:null")])).expect_err("state holds no deletion");
        assert!(matches!(refusal, EntityRefusal::NullInState { .. }), "{refusal:?}");
    }

    /// Clause 4.5.0: beside data the Null is refused in either mode.
    #[test]
    fn a_member_holding_the_null_beside_data_is_refused_in_either_mode() {
        let items = || structured(vec![text("a"), text("urn:ngsi-ld:null")]);
        let fragment = expanded_value::<Fragment>(&attribute(), items()).expect_err("the Null is no data");
        let stored = expanded_value::<Stored>(&attribute(), items()).expect_err("the Null is no data");
        assert!(matches!(fragment, EntityRefusal::NullBesideData { .. }), "{fragment:?}");
        assert!(matches!(stored, EntityRefusal::NullBesideData { .. }), "{stored:?}");
    }

    /// A node naming the Null beside other members is the Null beside data.
    #[test]
    fn a_node_naming_the_null_beside_members_is_refused() {
        let node = ExpandedItem::Node(ExpandedNode {
            id: Some("urn:ngsi-ld:null".to_owned()),
            members: vec![("https://example.org/level".to_owned(), vec![text("1")])],
            ..ExpandedNode::default()
        });
        let refusal = expanded_value::<Fragment>(&attribute(), node).expect_err("the Null is no node identifier");
        assert!(matches!(refusal, EntityRefusal::NullBesideData { .. }), "{refusal:?}");
    }
}
