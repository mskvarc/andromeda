//! What expansion makes of a request before it becomes a document.
//!
//! Both ways of expanding a request end here: the typed walk decides every
//! term against the processed context and writes this tree, and the JSON-LD
//! processor's own expanded document is read into it. One assembly then turns
//! the tree into the written document, so the two ways cannot disagree on
//! anything but the tree itself, which the differential tests compare.
//!
//! The tree keeps exactly what JSON-LD 1.1 expansion produces -- value
//! objects, JSON literals, node objects, list objects -- with every identifier
//! and IRI still the text expansion produced and every number the exact text
//! it was written with. One shape is added: a geometry the transport already
//! read into its positions stays those positions, so a `GeoProperty` of a
//! million positions never becomes a million value objects; its `bbox` and its
//! foreign members stand beside them, the foreign members expanded as the
//! members of any node.

use andromeda_ngsi_ld_entity::literal::Direction;
use andromeda_ngsi_ld_geometry::{bounding_box::BoundingBox, form::GeometryForm};
use serde_json::Value;

/// One expanded item: what JSON-LD 1.1 expansion yields in a value position.
#[derive(Clone, Debug, PartialEq)]
pub enum ExpandedItem {
    /// A value object (`@value`, with `@type`, `@language` or `@direction`).
    Value(ValueObject),
    /// A JSON literal (`"@type": "@json"`), carried verbatim.
    Json(Value),
    /// A node object or a node reference.
    Node(ExpandedNode),
    /// A list object (`@list`), in order.
    List(Vec<ExpandedItem>),
    /// A `GeoJSON` geometry the transport already read into its positions; it
    /// stands for the `GeoJSON` node expansion makes of that geometry.
    Geometry(Box<ExpandedGeometry>),
}

/// A `GeoJSON` geometry read into its positions, with the members written
/// beside them (IETF RFC 7946 sections 5 and 6.1).
#[derive(Clone, Debug, PartialEq)]
pub struct ExpandedGeometry {
    /// The positions.
    pub form: GeometryForm,
    /// The `bbox`, when one was written.
    pub bbox: Option<BoundingBox>,
    /// The foreign members by expanded name, each with its values in order.
    pub foreign: Vec<(String, Vec<ExpandedItem>)>,
}

impl ExpandedGeometry {
    /// A geometry of positions alone.
    #[must_use]
    pub const fn bare(form: GeometryForm) -> ExpandedGeometry {
        ExpandedGeometry {
            form,
            bbox: None,
            foreign: Vec::new(),
        }
    }
}

/// The scalar of a value object.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Scalar {
    /// A number, as the exact text it was written with.
    Number(String),
    /// A string.
    String(String),
    /// A boolean.
    Boolean(bool),
}

/// A value object.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValueObject {
    /// The value.
    pub scalar: Scalar,
    /// The expanded datatype IRI of `@type`.
    pub datatype: Option<String>,
    /// The `@language` tag.
    pub language: Option<String>,
    /// The `@direction`.
    pub direction: Option<Direction>,
}

impl ValueObject {
    /// An untyped, untagged value object.
    #[must_use]
    pub const fn plain(scalar: Scalar) -> ValueObject {
        ValueObject {
            scalar,
            datatype: None,
            language: None,
            direction: None,
        }
    }

    /// The string of the value object, when it holds a string.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match &self.scalar {
            Scalar::String(text) => Some(text),
            Scalar::Number(_) | Scalar::Boolean(_) => None,
        }
    }
}

/// A node object.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExpandedNode {
    /// The `@id`, as expansion produced it.
    pub id: Option<String>,
    /// The `@type` entries, as expanded IRIs, in order.
    pub types: Vec<String>,
    /// The members by expanded name, each with its values in order. A name
    /// two keys expanded to holds the values of both, in key order.
    pub members: Vec<(String, Vec<ExpandedItem>)>,
}

impl ExpandedNode {
    /// Append `values` to the member `name`, creating it when absent.
    pub fn add_member(&mut self, name: String, values: Vec<ExpandedItem>) {
        match self.members.iter_mut().find(|(existing, _)| *existing == name) {
            Some((_, existing)) => existing.extend(values),
            None => self.members.push((name, values)),
        }
    }

    /// Whether the node carries an identifier and nothing else.
    #[must_use]
    pub const fn is_reference(&self) -> bool {
        self.id.is_some() && self.types.is_empty() && self.members.is_empty()
    }

    /// The values of the member `name`, when the node carries it.
    #[must_use]
    pub fn member(&self, name: &str) -> Option<&[ExpandedItem]> {
        self.members.iter().find(|(existing, _)| existing == name).map(|(_, values)| values.as_slice())
    }
}

#[cfg(test)]
mod tests {
    use crate::expanded_tree::{ExpandedItem, ExpandedNode, Scalar, ValueObject};

    #[test]
    fn two_keys_expanding_to_one_name_share_one_member_in_key_order() {
        let mut node = ExpandedNode::default();
        let first = ExpandedItem::Value(ValueObject::plain(Scalar::Boolean(true)));
        let second = ExpandedItem::Value(ValueObject::plain(Scalar::Boolean(false)));
        node.add_member("https://ex.org/a".to_owned(), vec![first.clone()]); // clone: the expectation below compares against it
        node.add_member("https://ex.org/a".to_owned(), vec![second.clone()]); // clone: the expectation below compares against it

        pretty_assertions::assert_eq!(node.member("https://ex.org/a"), Some([first, second].as_slice()));
        pretty_assertions::assert_eq!(node.members.len(), 1);
    }

    #[test]
    fn a_node_with_only_an_identifier_is_a_reference() {
        let reference = ExpandedNode {
            id: Some("urn:ngsi-ld:Building:1".to_owned()),
            ..ExpandedNode::default()
        };
        assert!(reference.is_reference());
        assert!(!ExpandedNode::default().is_reference());
    }
}
