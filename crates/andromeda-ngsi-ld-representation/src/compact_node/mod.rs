//! One node of a compact NGSI-LD document, whatever holds it.
//!
//! A compact document reaches the broker as text (a request body, a peer's
//! answer, a notification), and the term walk of compaction writes one as a
//! tree of its own, in which a `GeoProperty` geometry stays the typed geometry
//! it was held as. The representation rules of this crate and the compact
//! model's reading of entities and attribute members are stated once over this
//! view, so the rules that decide what a member is -- an attribute instance of
//! which family, a concise value, a node no clause admits -- cannot differ
//! between a request and a compacted entity.
//!
//! A node is a borrowed handle, copied freely; looking at one yields the
//! handles of its elements or members, in the order they were written.

pub mod aliased_node;
mod piece_node;
pub mod produced_node;

use andromeda_json_literal::{JsonLiteral, admission::JsonLiteralError};
use andromeda_ngsi_ld_geometry::{form::GeometryForm, written_geometry::WrittenGeometry};
use andromeda_ngsi_ld_vocabulary::ngsi_ld_null::is_ngsi_ld_null;
use serde::de::{DeserializeOwned, value::BorrowedStrDeserializer};
use std::{borrow::Cow, error::Error, vec::IntoIter};

/// One node of a compact JSON document.
pub trait CompactNode<'n>: Copy + 'n {
    /// Why the node has no JSON form to read a value from.
    type Unreadable: Error + 'static;

    /// What the node is.
    fn shape(self) -> NodeShape<'n, Self>;

    /// The node as a JSON literal of its own.
    ///
    /// # Errors
    ///
    /// Returns [`CompactNode::Unreadable`] when the node has no JSON form; the
    /// inner result is the literal's admission.
    fn literal(self) -> Result<Result<JsonLiteral, JsonLiteralError>, Self::Unreadable>;

    /// The node decoded as a `T`, a string handed over as the string it is.
    ///
    /// # Errors
    ///
    /// Returns [`CompactNode::Unreadable`] when the node has no JSON form; the
    /// inner result is the decoding's own outcome.
    fn decoded<T: DeserializeOwned>(self) -> Result<Result<T, serde_json::Error>, Self::Unreadable>;

    /// The node read as a written `GeoJSON` geometry object (IETF RFC 7946
    /// section 3.1).
    ///
    /// # Errors
    ///
    /// Returns [`CompactNode::Unreadable`] when the node has no JSON form; the
    /// inner result is the geometry reader's outcome.
    fn read_geometry(self) -> Result<Result<WrittenGeometry, serde_json::Error>, Self::Unreadable>;

    /// The member of an object node named `name`; of a name written twice,
    /// the last.
    fn member(self, name: &str) -> Option<Self> {
        match self.shape() {
            NodeShape::Object(members) => members.get(name),
            NodeShape::Null | NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::String(_) | NodeShape::Array(_) | NodeShape::Geometry(_) => None,
        }
    }

    /// The string the node is, when it is one.
    fn text(self) -> Option<Cow<'n, str>> {
        match self.shape() {
            NodeShape::String(text) => Some(text),
            NodeShape::Null | NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::Array(_) | NodeShape::Object(_) | NodeShape::Geometry(_) => None,
        }
    }

    /// Whether the node is the NGSI-LD Null written as a string (clause
    /// 4.5.0).
    fn is_ngsi_ld_null(self) -> bool {
        self.text().is_some_and(|text| is_ngsi_ld_null(&text))
    }
}

/// What one compact node is.
#[derive(Debug)]
pub enum NodeShape<'n, N> {
    /// `null`.
    Null,
    /// `true` or `false`.
    Boolean(bool),
    /// A number, in the spelling it was written with.
    Number(Cow<'n, str>),
    /// A string, unescaped.
    String(Cow<'n, str>),
    /// An array, its elements in order.
    Array(Vec<N>),
    /// An object, its members in written order.
    Object(NodeMembers<'n, N>),
    /// A `GeoJSON` geometry object held as the typed geometry it spells.
    Geometry(&'n GeometryForm),
}

/// The members of an object node, in written order.
#[derive(Debug)]
pub struct NodeMembers<'n, N>(Vec<(Cow<'n, str>, N)>);

impl<'n, N: Copy> NodeMembers<'n, N> {
    /// The members `members` lists, in that order.
    #[must_use]
    pub const fn new(members: Vec<(Cow<'n, str>, N)>) -> NodeMembers<'n, N> {
        NodeMembers(members)
    }

    /// The member named `name`; of a name written twice, the last, as a
    /// parsed object keeps it.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<N> {
        self.0.iter().rev().find_map(|(member, node)| (member == name).then_some(*node))
    }

    /// Whether a member is named `name`.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.0.iter().any(|(member, _)| member == name)
    }

    /// How many members the object has.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the object has no member.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The members, in written order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, N)> {
        self.0.iter().map(|(name, node)| (name.as_ref(), *node))
    }
}

impl<'n, N> IntoIterator for NodeMembers<'n, N> {
    type IntoIter = IntoIter<(Cow<'n, str>, N)>;
    type Item = (Cow<'n, str>, N);

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

/// Decode a string node as a `T`, the string handed over borrowed, as the
/// text of a document hands it over: some types (a UN/CEFACT unit code) decode
/// only from a borrowed string.
///
/// # Errors
///
/// Returns the decoding's failure.
fn decoded_string<T: DeserializeOwned>(text: &str) -> Result<T, serde_json::Error> {
    T::deserialize(BorrowedStrDeserializer::new(text))
}

#[cfg(test)]
mod tests;
