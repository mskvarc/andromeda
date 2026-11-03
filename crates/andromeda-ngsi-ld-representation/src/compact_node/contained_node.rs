//! A compact node whose members are read as compaction writes them without
//! the containers of the terms they are written under.
//!
//! [`ContainerTerms`] says which terms a context reshapes. Under a `@set` term
//! an array of one is read as its one value; under a map container an object
//! holding one key is read as the value under it, the key being an index, an
//! identifier or a type the value states again; a map of several keys is read
//! as the array of its values in written order, the elements of a value that
//! is an array taking its place, which is what JSON-LD 1.1 expansion makes of
//! the map; the members under a `@nest` alias are read as members of the
//! object holding it. A value is never reshaped inside: what a node holds as
//! JSON is taken from the node as it was written.

use crate::{
    compact_node::{CompactNode, NodeMembers, NodeShape},
    container_terms::{ContainerTerms, TermContainer},
};
use andromeda_json_literal::{JsonLiteral, admission::JsonLiteralError};
use andromeda_ngsi_ld_geometry::written_geometry::WrittenGeometry;
use serde::de::DeserializeOwned;
use std::borrow::Cow;

/// What a node is read as.
#[derive(Debug, Clone, Copy)]
enum Reading {
    /// The node as it was written.
    AsWritten,
    /// The object of a map container with several keys, read as the array of
    /// its values.
    MapValues,
}

/// A compact node whose members are read without the containers of their
/// terms.
#[derive(Debug, Clone, Copy)]
pub struct ContainedNode<'a, N> {
    /// The node as written.
    node: N,
    /// The terms whose container reshapes what is compacted under them.
    containers: &'a ContainerTerms,
    /// What the node is read as.
    reading: Reading,
}

impl<'a, N> ContainedNode<'a, N> {
    /// Read `node` without the containers `containers` records.
    #[must_use]
    pub const fn new(node: N, containers: &'a ContainerTerms) -> ContainedNode<'a, N> {
        ContainedNode {
            node,
            containers,
            reading: Reading::AsWritten,
        }
    }

    /// `node`, read through the same containers.
    const fn nested(&self, node: N) -> ContainedNode<'a, N> {
        ContainedNode::new(node, self.containers)
    }

    /// `node`, the object of a map container, read as the array of its values.
    const fn map_values(&self, node: N) -> ContainedNode<'a, N> {
        ContainedNode {
            node,
            containers: self.containers,
            reading: Reading::MapValues,
        }
    }
}

impl<'a: 'n, 'n, N: CompactNode<'n>> ContainedNode<'a, N> {
    /// The members of the object `members` makes up, those under a `@nest`
    /// alias read as its own, each read without the container of its term.
    fn uncontained(self, members: NodeMembers<'n, N>, read: &mut Vec<(Cow<'n, str>, ContainedNode<'a, N>)>) {
        for (name, node) in members {
            match self.containers.container_of(&name) {
                Some(TermContainer::Nest) => match node.shape() {
                    NodeShape::Object(nested) => self.uncontained(nested, read),
                    NodeShape::Null | NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::String(_) | NodeShape::Array(_) | NodeShape::Geometry(_) => {
                        read.push((name, self.nested(node)));
                    }
                },
                Some(TermContainer::Set) => read.push((name, self.nested(single_element(node).unwrap_or(node)))),
                Some(TermContainer::Map) => read.push((name, self.map_member(node))),
                None => read.push((name, self.nested(node))),
            }
        }
    }

    /// The node of a member under a map container: the value under the one
    /// key of a map of one, the values of a map of several, and any other
    /// node as written.
    fn map_member(&self, node: N) -> ContainedNode<'a, N> {
        match node.shape() {
            NodeShape::Object(members) if members.len() == 1 => members.into_iter().next().map_or_else(|| self.nested(node), |(_, value)| self.nested(value)),
            NodeShape::Object(members) if members.len() > 1 => self.map_values(node),
            NodeShape::Null
            | NodeShape::Boolean(_)
            | NodeShape::Number(_)
            | NodeShape::String(_)
            | NodeShape::Array(_)
            | NodeShape::Object(_)
            | NodeShape::Geometry(_) => self.nested(node),
        }
    }

    /// The values of the map `members` make up, those that are arrays
    /// replaced by their elements.
    fn map_elements(&self, members: NodeMembers<'n, N>) -> Vec<ContainedNode<'a, N>> {
        members
            .into_iter()
            .flat_map(|(_, value)| match value.shape() {
                NodeShape::Array(items) => items,
                NodeShape::Null | NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::String(_) | NodeShape::Object(_) | NodeShape::Geometry(_) => {
                    vec![value]
                }
            })
            .map(|value| self.nested(value))
            .collect()
    }
}

/// The one element of the array `node` is, when it holds exactly one.
fn single_element<'n, N: CompactNode<'n>>(node: N) -> Option<N> {
    match node.shape() {
        NodeShape::Array(items) => match items.as_slice() {
            [only] => Some(*only),
            _ => None,
        },
        NodeShape::Null | NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::String(_) | NodeShape::Object(_) | NodeShape::Geometry(_) => None,
    }
}

impl<'a: 'n, 'n, N: CompactNode<'n>> CompactNode<'n> for ContainedNode<'a, N> {
    type Unreadable = N::Unreadable;

    fn shape(self) -> NodeShape<'n, ContainedNode<'a, N>> {
        match self.node.shape() {
            NodeShape::Null => NodeShape::Null,
            NodeShape::Boolean(flag) => NodeShape::Boolean(flag),
            NodeShape::Number(text) => NodeShape::Number(text),
            NodeShape::String(text) => NodeShape::String(text),
            NodeShape::Array(items) => NodeShape::Array(items.into_iter().map(|item| self.nested(item)).collect()),
            NodeShape::Object(members) => match self.reading {
                Reading::MapValues => NodeShape::Array(self.map_elements(members)),
                Reading::AsWritten => {
                    let mut read = Vec::with_capacity(members.len());
                    self.uncontained(members, &mut read);
                    NodeShape::Object(NodeMembers::new(read))
                }
            },
            NodeShape::Geometry(form) => NodeShape::Geometry(form),
        }
    }

    fn literal(self) -> Result<Result<JsonLiteral, JsonLiteralError>, N::Unreadable> {
        self.node.literal()
    }

    fn decoded<T: DeserializeOwned>(self) -> Result<Result<T, serde_json::Error>, N::Unreadable> {
        self.node.decoded()
    }

    fn read_geometry(self) -> Result<Result<WrittenGeometry, serde_json::Error>, N::Unreadable> {
        self.node.read_geometry()
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        compact_node::{CompactNode, NodeShape, contained_node::ContainedNode},
        container_terms::{ContainerTerms, TermContainer},
    };
    use andromeda_json_literal::piece::JsonPiece;
    use serde_json::value::RawValue;

    /// The containers a `@set` type alias, an `@index` map and a `@nest`
    /// alias give.
    fn containers() -> ContainerTerms {
        let mut containers = ContainerTerms::none();
        containers.contain("tps", TermContainer::Set);
        containers.contain("speed", TermContainer::Map);
        containers.contain("n", TermContainer::Nest);
        containers
    }

    /// The names of the members of `node` and whether each is a string.
    fn member_kinds<'n, N: CompactNode<'n>>(node: N) -> Vec<(String, &'static str)> {
        let NodeShape::Object(members) = node.shape() else {
            panic!("an object");
        };
        members
            .into_iter()
            .map(|(name, value)| {
                let kind = match value.shape() {
                    NodeShape::String(_) => "string",
                    NodeShape::Array(_) => "array",
                    NodeShape::Object(_) => "object",
                    NodeShape::Null | NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::Geometry(_) => "scalar",
                };
                (name.into_owned(), kind)
            })
            .collect()
    }

    #[test]
    fn an_array_of_one_under_a_set_term_is_its_value() {
        let text = RawValue::from_string(r#"{"tps": ["Property"], "other": ["x"]}"#.to_owned()).expect("JSON");
        let containers = containers();
        let node = ContainedNode::new(JsonPiece::new(&text), &containers);
        pretty_assertions::assert_eq!(member_kinds(node), vec![("tps".to_owned(), "string"), ("other".to_owned(), "array")]);
    }

    #[test]
    fn a_map_of_one_key_is_the_value_under_it() {
        let text = RawValue::from_string(r#"{"speed": {"@none": {"type": "Property", "value": 50}}}"#.to_owned()).expect("JSON");
        let containers = containers();
        let node = ContainedNode::new(JsonPiece::new(&text), &containers);
        let speed = node.member("speed").expect("the member");
        pretty_assertions::assert_eq!(speed.member("value").map(|value| matches!(value.shape(), NodeShape::Number(_))), Some(true));
    }

    #[test]
    fn the_members_under_a_nest_alias_belong_to_its_object() {
        let text = RawValue::from_string(r#"{"id": "urn:x", "n": {"brand": "a", "n": {"model": "b"}}}"#.to_owned()).expect("JSON");
        let containers = containers();
        let node = ContainedNode::new(JsonPiece::new(&text), &containers);
        pretty_assertions::assert_eq!(
            member_kinds(node),
            vec![("id".to_owned(), "string"), ("brand".to_owned(), "string"), ("model".to_owned(), "string")]
        );
    }

    /// The numbers the elements of the array `node` is hold, in order.
    fn numbers<'n, N: CompactNode<'n>>(node: N) -> Vec<String> {
        let NodeShape::Array(items) = node.shape() else {
            panic!("an array");
        };
        items
            .into_iter()
            .map(|item| match item.shape() {
                NodeShape::Number(text) => text.into_owned(),
                NodeShape::Null | NodeShape::Boolean(_) | NodeShape::String(_) | NodeShape::Array(_) | NodeShape::Object(_) | NodeShape::Geometry(_) => {
                    panic!("a number")
                }
            })
            .collect()
    }

    #[test]
    fn a_map_of_two_keys_is_the_array_of_its_values_in_written_order() {
        let text = RawValue::from_string(r#"{"speed": {"b": 1, "a": 2}}"#.to_owned()).expect("JSON");
        let containers = containers();
        let node = ContainedNode::new(JsonPiece::new(&text), &containers);
        pretty_assertions::assert_eq!(numbers(node.member("speed").expect("the member")), vec!["1".to_owned(), "2".to_owned()]);
    }

    #[test]
    fn a_map_key_holding_an_array_gives_its_elements_their_places() {
        let text = RawValue::from_string(r#"{"speed": {"a": [1, 2], "b": 3, "c": [4]}}"#.to_owned()).expect("JSON");
        let containers = containers();
        let node = ContainedNode::new(JsonPiece::new(&text), &containers);
        pretty_assertions::assert_eq!(
            numbers(node.member("speed").expect("the member")),
            vec!["1".to_owned(), "2".to_owned(), "3".to_owned(), "4".to_owned()]
        );
    }

    #[test]
    fn a_map_of_one_key_holding_an_array_is_that_array() {
        let text = RawValue::from_string(r#"{"speed": {"a": [1, 2]}}"#.to_owned()).expect("JSON");
        let containers = containers();
        let node = ContainedNode::new(JsonPiece::new(&text), &containers);
        pretty_assertions::assert_eq!(numbers(node.member("speed").expect("the member")), vec!["1".to_owned(), "2".to_owned()]);
    }
}
