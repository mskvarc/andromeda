//! Whether two pieces spell the same JSON value.
//!
//! IETF RFC 8259 section 4 makes an object "an unordered collection", so
//! member order is ignored; clause 4.6.3 keeps a number's mathematical value,
//! so numbers compare by value. Arrays are ordered and compare element by
//! element. Admission bounds the nesting, which bounds the recursion.

use crate::piece::{JsonKind, JsonPiece};

// Hand-written: two pieces are equal when the JSON values they spell are, which
// no derive over the text expresses.
impl PartialEq for JsonPiece<'_> {
    fn eq(&self, other: &JsonPiece<'_>) -> bool {
        same_value(*self, *other)
    }
}

impl Eq for JsonPiece<'_> {}

/// Whether `left` and `right` are the same JSON value.
pub(crate) fn same_value(left: JsonPiece<'_>, right: JsonPiece<'_>) -> bool {
    match (left.kind(), right.kind()) {
        (JsonKind::Null, JsonKind::Null) => true,
        (JsonKind::Boolean, JsonKind::Boolean) => left.as_bool() == right.as_bool(),
        (JsonKind::Number, JsonKind::Number) => left.as_number() == right.as_number(),
        (JsonKind::String, JsonKind::String) => left.as_str() == right.as_str(),
        (JsonKind::Array, JsonKind::Array) => match (left.elements(), right.elements()) {
            (Some(left), Some(right)) => left.len() == right.len() && left.into_iter().zip(right).all(|(left, right)| same_value(left, right)),
            (Some(_) | None, Some(_) | None) => false,
        },
        (JsonKind::Object, JsonKind::Object) => match (left.members(), right.members()) {
            (Some(left), Some(right)) => {
                left.len() == right.len()
                    && left.iter().all(|(name, value)| {
                        right
                            .iter()
                            .find(|(other, _)| other == name)
                            .is_some_and(|(_, other)| same_value(*value, *other))
                    })
            }
            (Some(_) | None, Some(_) | None) => false,
        },
        (
            JsonKind::Null | JsonKind::Boolean | JsonKind::Number | JsonKind::String | JsonKind::Array | JsonKind::Object,
            JsonKind::Null | JsonKind::Boolean | JsonKind::Number | JsonKind::String | JsonKind::Array | JsonKind::Object,
        ) => false,
    }
}
