//! One expanded value as the `q` evaluator tells values apart, and the members
//! of a structured value a compound path descends into (clause 4.9).

use crate::{
    expanded_value::{ExpandedValue, ValueNode},
    literal::LiteralValue,
    node_id::NodeId,
    queried::queried_mode::QueriedMode,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::queried_entity::{attribute_reading::QueriedValueNode, value_item};
use std::borrow::Cow;

/// The item one expanded value is to the evaluator.
///
/// A literal is compared by its value alone, its datatype and language are not
/// part of what clause 4.9 compares; a JSON literal is raw JSON addressed by its
/// verbatim keys; a node is a structured value. A nested list carries neither a
/// literal nor addressable members.
#[must_use]
pub fn item_of<M: QueriedMode>(value: &ExpandedValue<M>) -> value_item::ValueItem<'_, &ValueNode<M>> {
    match value {
        ExpandedValue::Literal(literal) => value_item::ValueItem::Literal(literal_value(literal.value())),
        ExpandedValue::Json(json) => value_item::ValueItem::Json(json.piece()),
        ExpandedValue::Node(node) => value_item::ValueItem::Node(node),
        ExpandedValue::List(_) => value_item::ValueItem::Opaque,
    }
}

/// A literal's value in the three JSON scalar types clause 4.9 compares.
#[must_use]
pub fn literal_value(value: &LiteralValue) -> value_item::LiteralValue<'_> {
    match value {
        LiteralValue::Number(number) => value_item::LiteralValue::Number(Cow::Borrowed(number)),
        LiteralValue::String(text) => value_item::LiteralValue::Text(Cow::Borrowed(text.as_str())),
        LiteralValue::Boolean(boolean) => value_item::LiteralValue::Boolean(*boolean),
    }
}

impl<'a, M: QueriedMode> QueriedValueNode<'a> for &'a ValueNode<M> {
    fn id(self) -> Option<&'a str> {
        ValueNode::id(self).map(NodeId::as_str)
    }

    fn first_member_value(self, member: &AttributeName) -> Option<value_item::ValueItem<'a, &'a ValueNode<M>>> {
        self.members()
            .get(member.as_str())
            .and_then(|values| M::held_values(values).first())
            .map(item_of)
    }
}
