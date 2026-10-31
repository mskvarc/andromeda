//! One expanded value as the `q` evaluator tells values apart, and the members
//! of a structured value a compound path descends into (clause 4.9).

use crate::{
    expanded_value::{ExpandedValue, ValueNode},
    literal::{Literal, LiteralValue},
    node_id::NodeId,
    queried::queried_mode::QueriedMode,
};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    reference::NgsiLdReference,
    time::value::{TimeValue, TimeValueDeclaration},
};
use andromeda_ngsi_ld_query::queried_entity::{attribute_reading::QueriedValueNode, value_item};
use std::borrow::Cow;

/// The item one expanded value is to the evaluator, `value_type` being the
/// `valueType` beside it (`None` inside a structured value).
///
/// A literal is compared by its value; its datatype counts only where it, or
/// the `valueType`, declares a time value, and its language not at all. A JSON
/// literal is raw JSON addressed by its verbatim keys; a node is a structured
/// value. A nested list carries neither a literal nor addressable members.
#[must_use]
pub fn item_of<'a, M: QueriedMode>(value: &'a ExpandedValue<M>, value_type: Option<&NgsiLdReference>) -> value_item::ValueItem<'a, &'a ValueNode<M>> {
    match value {
        ExpandedValue::Literal(literal) => value_item::ValueItem::Literal(declared_literal_value(literal, value_type)),
        ExpandedValue::Json(json) => value_item::ValueItem::Json(json.piece()),
        ExpandedValue::Node(node) => value_item::ValueItem::Node(node),
        ExpandedValue::List(_) => value_item::ValueItem::Opaque,
    }
}

/// A literal's value as clause 4.9 compares it: the time value it is declared
/// as, else its JSON scalar.
fn declared_literal_value<'a>(literal: &'a Literal, value_type: Option<&NgsiLdReference>) -> value_item::LiteralValue<'a> {
    match TimeValue::declared(TimeValueDeclaration::new(literal.datatype(), value_type), literal.value().primitive()) {
        Some(Ok(value)) => value_item::LiteralValue::Time(value),
        Some(Err(_)) | None => literal_value(literal.value()),
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

    fn member_values(self, member: &AttributeName) -> impl Iterator<Item = value_item::ValueItem<'a, &'a ValueNode<M>>> {
        self.members()
            .get(member.as_str())
            .map(|values| M::held_values(values))
            .into_iter()
            .flatten()
            .map(|value| item_of(value, None))
    }
}
