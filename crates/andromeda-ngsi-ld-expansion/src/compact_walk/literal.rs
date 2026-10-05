//! Expanding one scalar under the term it is written under (JSON-LD 1.1
//! Value Expansion algorithm, section 5.3.2).

use crate::{
    decline::Decline,
    expanded_tree::{ExpandedItem, ExpandedNode, Scalar, ValueObject},
    term_scope::{TermScope, identifier_text},
};
use andromeda_ngsi_ld_entity::literal::Direction;
use andromeda_ngsi_ld_representation::compact_node::{CompactNode, NodeShape};
use jsonld::{Direction as JsonLdDirection, Nullable, Type};

/// Expand the scalar `value` written under `property`.
///
/// # Errors
///
/// Declines a type mapping the algorithm refuses for a scalar.
pub fn expand_scalar<'n, N: CompactNode<'n>>(scope: &TermScope<'_>, property: &str, value: N) -> Result<ExpandedItem, Decline> {
    let definition = scope.definition(property);
    let type_mapping = definition.and_then(|definition| definition.typ());
    match (type_mapping, value.shape()) {
        // An `@id`-typed term makes a string a node reference (step 1).
        (Some(Type::Id), NodeShape::String(text)) => Ok(node_reference(identifier_text(scope.expand_id(&text)?.as_ref()))),
        // A `@vocab`-typed term expands it with the vocabulary mapping (step 2).
        (Some(Type::Vocab), NodeShape::String(text)) => Ok(node_reference(identifier_text(scope.expand_vocab(&text)?.as_ref()))),
        (Some(Type::Iri(datatype)), shape) => Ok(ExpandedItem::Value(ValueObject {
            scalar: scalar(shape)?,
            datatype: Some(datatype.as_str().to_owned()),
            language: None,
            direction: None,
        })),
        (Some(Type::Json), _) => Err(Decline::MalformedJsonLd),
        (None | Some(Type::None), NodeShape::String(text)) => {
            // A string takes the term's language and direction, or the
            // context's defaults (steps 5 and 6).
            let language = match definition.and_then(|definition| definition.language()) {
                Some(Nullable::Some(tag)) => Some(tag.as_str().to_owned()),
                Some(Nullable::Null) => None,
                None => scope.default_language(),
            };
            let direction = match definition.and_then(|definition| definition.direction()) {
                Some(Nullable::Some(direction)) => Some(direction),
                Some(Nullable::Null) => None,
                None => scope.default_direction(),
            };
            Ok(ExpandedItem::Value(ValueObject {
                scalar: Scalar::String(text.into_owned()),
                datatype: None,
                language,
                direction: direction.map(direction_of),
            }))
        }
        (
            None | Some(Type::Id | Type::Vocab | Type::None),
            shape @ (NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::Null | NodeShape::Array(_) | NodeShape::Object(_) | NodeShape::Geometry(_)),
        ) => scalar(shape).map(|scalar| ExpandedItem::Value(ValueObject::plain(scalar))),
    }
}

/// The scalar of a JSON value; numbers keep the exact text they were written
/// with.
///
/// # Errors
///
/// Declines a value that is no scalar.
pub fn scalar<N>(shape: NodeShape<'_, N>) -> Result<Scalar, Decline> {
    match shape {
        NodeShape::Boolean(flag) => Ok(Scalar::Boolean(flag)),
        NodeShape::Number(text) => Ok(Scalar::Number(text.into_owned())),
        NodeShape::String(text) => Ok(Scalar::String(text.into_owned())),
        NodeShape::Null | NodeShape::Array(_) | NodeShape::Object(_) | NodeShape::Geometry(_) => Err(Decline::MalformedJsonLd),
    }
}

/// The node reference an `@id`- or `@vocab`-coerced string expands to.
fn node_reference(id: Option<String>) -> ExpandedItem {
    ExpandedItem::Node(ExpandedNode { id, ..ExpandedNode::default() })
}

/// The base direction in the model's terms.
pub const fn direction_of(direction: JsonLdDirection) -> Direction {
    match direction {
        JsonLdDirection::Ltr => Direction::Ltr,
        JsonLdDirection::Rtl => Direction::Rtl,
    }
}
