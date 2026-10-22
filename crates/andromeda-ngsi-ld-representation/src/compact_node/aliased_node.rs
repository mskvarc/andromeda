//! A compact node whose reserved members are read under their core names.
//!
//! A document compacted under a context that defines another term for a
//! reserved NGSI-LD member -- an alias of `@type`, a second term for
//! `ngsi-ld:hasValue` -- writes the member under that term (JSON-LD 1.1 API
//! section 6.2.2 term selection, clause 4.5.0). The data is the same, so it is
//! read the same: each member name the context made an alias of a core term is
//! read as that core term, and so is a `type` naming a core class through an
//! alias (a context term for `ngsi-ld:Property`). A value is never renamed
//! inside: what a node holds as JSON is taken from the node as it was written.

use crate::compact_node::{CompactNode, NodeMembers, NodeShape};
use andromeda_json_literal::{JsonLiteral, admission::JsonLiteralError};
use andromeda_ngsi_ld_geometry::written_geometry::WrittenGeometry;
use andromeda_ngsi_ld_vocabulary::keyword;
use serde::de::DeserializeOwned;
use std::{borrow::Cow, collections::HashMap};

/// The terms a context defines for reserved NGSI-LD members, each mapped to
/// the core term it stands for.
#[derive(Debug, Clone, Default)]
pub struct CoreTermAliases(HashMap<Box<str>, &'static str>);

impl CoreTermAliases {
    /// No alias: every member is read under the name it is written with.
    #[must_use]
    pub fn none() -> CoreTermAliases {
        CoreTermAliases(HashMap::new())
    }

    /// Read the member written `alias` as the core term `core`.
    pub fn alias(&mut self, alias: &str, core: &'static str) {
        if alias != core {
            self.0.insert(Box::from(alias), core);
        }
    }

    /// Whether no member is read under another name.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The core term a member written `name` stands for.
    #[must_use]
    pub fn core_name<'n>(&self, name: Cow<'n, str>) -> Cow<'n, str> {
        match self.0.get(name.as_ref()) {
            Some(core) => Cow::Borrowed(core),
            None => name,
        }
    }
}

/// What an aliased node stands for, which decides what of it is read through
/// the aliases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Standing {
    /// Any node: the names of its members are read through the aliases.
    Node,
    /// The value of a `type` member: its strings are read through them too.
    Types,
}

/// A compact node whose members are read under the core terms their names
/// alias.
#[derive(Debug, Clone, Copy)]
pub struct AliasedNode<'a, N> {
    /// The node as written.
    node: N,
    /// The aliases its members are read through.
    aliases: &'a CoreTermAliases,
    /// What the node stands for.
    standing: Standing,
}

impl<'a, N> AliasedNode<'a, N> {
    /// Read `node` through `aliases`.
    #[must_use]
    pub const fn new(node: N, aliases: &'a CoreTermAliases) -> AliasedNode<'a, N> {
        AliasedNode {
            node,
            aliases,
            standing: Standing::Node,
        }
    }

    /// Read `node`, written in the place `standing` names, through the same
    /// aliases.
    fn standing(self, node: N, standing: Standing) -> AliasedNode<'a, N> {
        AliasedNode {
            node,
            aliases: self.aliases,
            standing,
        }
    }
}

impl<'a: 'n, 'n, N: CompactNode<'n>> CompactNode<'n> for AliasedNode<'a, N> {
    type Unreadable = N::Unreadable;

    fn shape(self) -> NodeShape<'n, AliasedNode<'a, N>> {
        match self.node.shape() {
            NodeShape::Null => NodeShape::Null,
            NodeShape::Boolean(flag) => NodeShape::Boolean(flag),
            NodeShape::Number(text) => NodeShape::Number(text),
            NodeShape::String(text) => NodeShape::String(match self.standing {
                Standing::Types => self.aliases.core_name(text),
                Standing::Node => text,
            }),
            NodeShape::Array(items) => NodeShape::Array(items.into_iter().map(|item| self.standing(item, self.standing)).collect()),
            NodeShape::Object(members) => NodeShape::Object(NodeMembers::new(
                members
                    .into_iter()
                    .map(|(name, node)| {
                        let name = self.aliases.core_name(name);
                        let standing = if name == keyword::TYPE_ALIAS { Standing::Types } else { Standing::Node };
                        (name, self.standing(node, standing))
                    })
                    .collect(),
            )),
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
