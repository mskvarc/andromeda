//! The `@id` of a node inside a structured value.
//!
//! JSON-LD names a node either by an IRI or by a blank node identifier
//! (`_:` followed by a label, JSON-LD 1.1 section 9.4). A structured Property
//! value that references another resource carries the IRI; a node that only
//! groups members may carry a blank node identifier.

use andromeda_ngsi_ld_lexicon::uri::Uri;
use compact_str::CompactString;
use derive_more::Display;
use std::str::FromStr;
use thiserror::Error;

/// The prefix of a blank node identifier.
const BLANK_NODE_PREFIX: &str = "_:";

/// A text that names no node.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("a node identifier is neither an IRI nor a blank node identifier")]
pub struct NodeIdError;

/// The identifier of a node.
#[derive(Clone, Debug, Display, Eq, Hash, PartialEq)]
pub enum NodeId {
    /// An absolute IRI.
    Uri(Uri),
    /// A blank node identifier, `_:` prefix included.
    Blank(CompactString),
}

impl NodeId {
    /// The identifier as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            NodeId::Uri(uri) => uri.as_str(),
            NodeId::Blank(label) => label,
        }
    }
}

impl FromStr for NodeId {
    type Err = NodeIdError;

    fn from_str(text: &str) -> Result<NodeId, NodeIdError> {
        match text.strip_prefix(BLANK_NODE_PREFIX) {
            Some(label) if !label.is_empty() => Ok(NodeId::Blank(CompactString::from(text))),
            Some(_) => Err(NodeIdError),
            None => text.parse::<Uri>().map(NodeId::Uri).map_err(|_| NodeIdError),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::node_id::{NodeId, NodeIdError};

    #[test]
    fn an_iri_and_a_blank_node_are_both_node_identifiers() {
        assert!(matches!("urn:ngsi-ld:Building:1".parse::<NodeId>(), Ok(NodeId::Uri(_))));
        assert!(matches!("_:b0".parse::<NodeId>(), Ok(NodeId::Blank(_))));
        pretty_assertions::assert_eq!("_:b0".parse::<NodeId>().map(|id| id.to_string()), Ok("_:b0".to_owned()));
    }

    #[test]
    fn an_empty_blank_label_is_refused() {
        pretty_assertions::assert_eq!("_:".parse::<NodeId>(), Err(NodeIdError));
    }
}
