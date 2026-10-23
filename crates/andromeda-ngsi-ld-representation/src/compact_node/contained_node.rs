//! A compact node whose members are read as compaction writes them without
//! the containers of the terms they are written under.
//!
//! [`ContainerTerms`] says which terms a context reshapes. Under a `@set` term
//! an array of one is read as its one value; under a map container an object
//! holding one key is read as the value under it, the key being an index, an
//! identifier or a type the value states again; the members under a `@nest`
//! alias are read as members of the object holding it. A map of several keys
//! is read as written, since its values belong to one member and no single
//! node stands for them. A value is never reshaped inside: what a node holds as
//! JSON is taken from the node as it was written.

use crate::{
    compact_node::{CompactNode, NodeMembers, NodeShape},
    container_terms::{ContainerTerms, TermContainer},
};
use andromeda_json_literal::{JsonLiteral, admission::JsonLiteralError};
use andromeda_ngsi_ld_geometry::written_geometry::WrittenGeometry;
use serde::de::DeserializeOwned;
use std::borrow::Cow;

/// A compact node whose members are read without the containers of their
/// terms.
#[derive(Debug, Clone, Copy)]
pub struct ContainedNode<'a, N> {
    /// The node as written.
    node: N,
    /// The terms whose container reshapes what is compacted under them.
    containers: &'a ContainerTerms,
}

impl<'a, N> ContainedNode<'a, N> {
    /// Read `node` without the containers `containers` records.
    #[must_use]
    pub const fn new(node: N, containers: &'a ContainerTerms) -> ContainedNode<'a, N> {
        ContainedNode { node, containers }
    }

    /// `node`, read through the same containers.
    const fn nested(&self, node: N) -> ContainedNode<'a, N> {
        ContainedNode {
            node,
            containers: self.containers,
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
                Some(TermContainer::Map) => read.push((name, self.nested(single_value(node).unwrap_or(node)))),
                None => read.push((name, self.nested(node))),
            }
        }
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

/// The value under the one key of the object `node` is, when it holds exactly
/// one.
fn single_value<'n, N: CompactNode<'n>>(node: N) -> Option<N> {
    match node.shape() {
        NodeShape::Object(members) if members.len() == 1 => members.into_iter().next().map(|(_, value)| value),
        NodeShape::Null
        | NodeShape::Boolean(_)
        | NodeShape::Number(_)
        | NodeShape::String(_)
        | NodeShape::Array(_)
        | NodeShape::Object(_)
        | NodeShape::Geometry(_) => None,
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
            NodeShape::Object(members) => {
                let mut read = Vec::with_capacity(members.len());
                self.uncontained(members, &mut read);
                NodeShape::Object(NodeMembers::new(read))
            }
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

    /// A map of several keys holds the values of one member, which no single
    /// node stands for.
    #[test]
    fn a_map_of_several_keys_is_read_as_written() {
        let text = RawValue::from_string(r#"{"speed": {"a": 1, "b": 2}}"#.to_owned()).expect("JSON");
        let containers = containers();
        let node = ContainedNode::new(JsonPiece::new(&text), &containers);
        pretty_assertions::assert_eq!(member_kinds(node), vec![("speed".to_owned(), "object")]);
    }
}
