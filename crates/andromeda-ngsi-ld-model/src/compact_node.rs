//! A compact JSON value as the model reads entities and their attribute
//! members from it.
//!
//! The compact model is read from two kinds of tree: a parsed JSON document,
//! and the document the term walk of compaction writes straight from entity
//! state, in which a `GeoProperty` geometry stays the typed geometry it was
//! held as instead of being written out position by position. Both are read by
//! the one reading of the model ([`Entity::from_node`](crate::entity::Entity),
//! [`AttributeValue::from_node`](crate::attribute::instance::AttributeValue)),
//! so the rules that decide what a member becomes -- an attribute instance of
//! which family, a multi-attribute, a member carried verbatim -- cannot differ
//! between a decoded document and a compacted entity.
//!
//! Wherever the model holds a value as JSON, it holds it in the spelling
//! `serde_json`'s reader gives it ([`CompactNode::to_json`]), which is what a
//! document decoded from its text holds.

use andromeda_ngsi_ld_geometry::{form::GeometryForm, written_geometry::WrittenGeometry};
use andromeda_ngsi_ld_number::json_value::deserialize_exact_document;
use serde::Deserialize;
use serde_json::{Map, Value, map::Iter as MemberIter};
use std::{error::Error, iter::Map as Mapped};

/// One node of a compact JSON tree.
///
/// An object node yields its members in the order of their names, which is the
/// order a decoded document's members come in.
pub trait CompactNode: Sized {
    /// The members of an object node, by name.
    type Members<'n>: Iterator<Item = (&'n str, &'n Self)>
    where
        Self: 'n;

    /// Why the node cannot be read as the JSON value it stands for.
    type Unreadable: Error + 'static;

    /// What the node is.
    fn shape(&self) -> NodeShape<'_, Self>;

    /// The member of an object node named `name`.
    fn member(&self, name: &str) -> Option<&Self>;

    /// The node as the JSON value it stands for, in the spelling
    /// `serde_json`'s reader gives it.
    ///
    /// # Errors
    ///
    /// Returns [`CompactNode::Unreadable`] when the node has no JSON form the
    /// reader accepts.
    fn to_json(&self) -> Result<Value, Self::Unreadable>;

    /// The node read as a written `GeoJSON` geometry object (IETF RFC 7946
    /// section 3.1), or the reader's failure to read it as one.
    ///
    /// # Errors
    ///
    /// Returns [`CompactNode::Unreadable`] when the node has no JSON form the
    /// reader accepts.
    fn read_geometry(&self) -> Result<Result<WrittenGeometry, serde_json::Error>, Self::Unreadable>;
}

/// What one compact node is.
#[derive(Debug)]
pub enum NodeShape<'n, N: CompactNode + 'n> {
    /// `null`.
    Null,
    /// `true` or `false`.
    Boolean,
    /// A number.
    Number,
    /// A string.
    String(&'n str),
    /// An array of nodes.
    Array(&'n [N]),
    /// An object, with its members by name.
    Object(N::Members<'n>),
    /// A `GeoJSON` geometry object held as the typed geometry it spells.
    Geometry(&'n GeometryForm),
}

/// The members of a JSON object, by name.
type JsonMembers<'n> = Mapped<MemberIter<'n>, fn((&'n String, &'n Value)) -> (&'n str, &'n Value)>;

/// A member of a JSON object with its name borrowed as a string slice.
const fn named_member<'n>((name, value): (&'n String, &'n Value)) -> (&'n str, &'n Value) {
    (name.as_str(), value)
}

impl CompactNode for Value {
    type Members<'n> = JsonMembers<'n>;
    type Unreadable = serde_json::Error;

    fn shape(&self) -> NodeShape<'_, Value> {
        match self {
            Value::Null => NodeShape::Null,
            Value::Bool(_) => NodeShape::Boolean,
            Value::Number(_) => NodeShape::Number,
            Value::String(text) => NodeShape::String(text),
            Value::Array(items) => NodeShape::Array(items),
            Value::Object(members) => NodeShape::Object(members.iter().map(named_member as fn(_) -> _)),
        }
    }

    fn member(&self, name: &str) -> Option<&Value> {
        self.as_object().and_then(|members: &Map<String, Value>| members.get(name))
    }

    fn to_json(&self) -> Result<Value, serde_json::Error> {
        // clone: the model owns the value it holds as JSON, the document keeps its own
        deserialize_exact_document(self.clone())
    }

    fn read_geometry(&self) -> Result<Result<WrittenGeometry, serde_json::Error>, serde_json::Error> {
        Ok(WrittenGeometry::deserialize(self))
    }
}

#[cfg(test)]
mod tests {
    use crate::compact_node::{CompactNode, NodeShape};
    use serde_json::{Number, Value, json};

    #[test]
    fn an_object_yields_its_members_in_the_order_of_their_names() {
        let object = json!({"b": 1, "a": 2, "c": 3});
        let NodeShape::Object(members) = object.shape() else {
            panic!("an object is an object node");
        };
        pretty_assertions::assert_eq!(members.map(|(name, _)| name).collect::<Vec<_>>(), vec!["a", "b", "c"]);
    }

    /// A number the reader would spell otherwise is held in the reader's
    /// spelling, as a document decoded from its text holds it.
    #[test]
    fn a_number_is_held_in_the_spelling_the_reader_gives_it() {
        let written = Value::Array(vec![Value::Number(Number::from_string_unchecked("1E+2".to_owned()))]);
        let reread: Value = serde_json::from_str(&written.to_string()).expect("the written array reads back");
        pretty_assertions::assert_eq!(written.to_json().expect("the array is readable"), reread);
    }
}
