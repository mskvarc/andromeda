//! A compacted document read as the compact model reads any compact tree.
//!
//! The compact model's reading ([`CompactNode`]) is the one a decoded document
//! gets, so a compacted entity becomes exactly the [`Entity`] its JSON would
//! decode to. Values the model holds as JSON are handed over in the spelling
//! `serde_json`'s reader gives them, as the decoding of the document's text
//! holds them; a geometry the walk kept typed is handed over as the typed
//! geometry it is, and has no JSON form here.
//!
//! [`Entity`]: andromeda_ngsi_ld_model::entity::Entity

use crate::compacted::{Compacted, UnwrittenGeometry};
use andromeda_ngsi_ld_geometry::written_geometry::WrittenGeometry;
use andromeda_ngsi_ld_model::compact_node::{CompactNode, NodeShape};
use andromeda_ngsi_ld_number::json_value::deserialize_exact_document;
use serde::Deserialize;
use serde_json::Value;
use std::{collections::btree_map::Iter as MemberIter, iter::Map as Mapped};
use thiserror::Error;

/// Why a node of a compacted document is read only from its written form.
#[derive(Debug, Error)]
pub(crate) enum UnreadableNode {
    /// The node holds a geometry that has not been written out.
    #[error(transparent)]
    Unwritten(#[from] UnwrittenGeometry),
    /// `serde_json`'s reader does not accept the node's JSON.
    #[error("the compacted JSON is not one serde_json reads: {0}")]
    Reader(#[source] serde_json::Error),
}

/// The members of a compacted object, by name.
type CompactedMembers<'n, 'a> = Mapped<MemberIter<'n, String, Compacted<'a>>, fn((&'n String, &'n Compacted<'a>)) -> (&'n str, &'n Compacted<'a>)>;

/// A member of a compacted object with its name borrowed as a string slice.
const fn named_member<'n, 'a>((name, value): (&'n String, &'n Compacted<'a>)) -> (&'n str, &'n Compacted<'a>) {
    (name.as_str(), value)
}

impl<'a> CompactNode for Compacted<'a> {
    type Members<'n>
        = CompactedMembers<'n, 'a>
    where
        Compacted<'a>: 'n;
    type Unreadable = UnreadableNode;

    fn shape(&self) -> NodeShape<'_, Compacted<'a>> {
        match self {
            Compacted::Null => NodeShape::Null,
            Compacted::Boolean(_) => NodeShape::Boolean,
            Compacted::Number(_) => NodeShape::Number,
            Compacted::String(text) => NodeShape::String(text),
            Compacted::Array(items) => NodeShape::Array(items),
            Compacted::Object(members) => NodeShape::Object(members.iter().map(named_member as fn(_) -> _)),
            Compacted::Geometry(form) => NodeShape::Geometry(form),
        }
    }

    fn member(&self, name: &str) -> Option<&Compacted<'a>> {
        match self {
            Compacted::Object(members) => members.get(name),
            Compacted::Null | Compacted::Boolean(_) | Compacted::Number(_) | Compacted::String(_) | Compacted::Array(_) | Compacted::Geometry(_) => None,
        }
    }

    fn to_json(&self) -> Result<Value, UnreadableNode> {
        // clone: the compact model owns the value it holds as JSON, the tree keeps its own
        deserialize_exact_document(self.clone().into_written_json()?).map_err(UnreadableNode::Reader)
    }

    fn read_geometry(&self) -> Result<Result<WrittenGeometry, serde_json::Error>, UnreadableNode> {
        self.to_json().map(|json| WrittenGeometry::deserialize(&json))
    }
}
