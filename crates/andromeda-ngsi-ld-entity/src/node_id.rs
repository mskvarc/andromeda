//! The `@id` of a node inside a structured value.
//!
//! JSON-LD names a node either by an IRI or by a blank node identifier
//! (`_:` followed by a label, JSON-LD 1.1 section 9.4). A structured Property
//! value that references another resource carries the IRI; a node that only
//! groups members may carry a blank node identifier.
//!
//! A Property value is any JSON object (clauses 3.1 and 4.6.3), and the core
//! `@context` aliases `id` to `@id`, so `{"id": "abc"}` written as a value is a
//! node whose `@id` is `abc`. An NGSI-LD document has no base IRI, so IRI
//! expansion leaves such a relative reference, or any text that is no IRI at
//! all, exactly as written (JSON-LD 1.1 IRI expansion, document-relative with
//! no base), and compaction returns it unchanged. Clause 4.6.4 requires the
//! value back as written, so that text is an identifier too.

use andromeda_ngsi_ld_lexicon::uri::Uri;
use compact_str::CompactString;
use derive_more::Display;

/// The prefix of a blank node identifier.
const BLANK_NODE_PREFIX: &str = "_:";

/// The identifier of a node.
#[derive(Clone, Debug, Display, Eq, Hash, PartialEq)]
pub enum NodeId {
    /// An absolute IRI.
    Uri(Uri),
    /// A blank node identifier, `_:` prefix included.
    Blank(CompactString),
    /// Any other text, which expansion keeps as written: a relative IRI
    /// reference resolved against no base, or a string that is no IRI.
    Verbatim(CompactString),
}

impl NodeId {
    /// The identifier as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            NodeId::Uri(uri) => uri.as_str(),
            NodeId::Blank(label) | NodeId::Verbatim(label) => label,
        }
    }
}

// Every text is an identifier, so the conversion cannot fail; it classifies
// the text, which no derive expresses.
impl From<&str> for NodeId {
    fn from(text: &str) -> NodeId {
        match text.strip_prefix(BLANK_NODE_PREFIX) {
            Some(label) if !label.is_empty() => NodeId::Blank(CompactString::from(text)),
            Some(_) | None => text.parse::<Uri>().map_or_else(|_| NodeId::Verbatim(CompactString::from(text)), NodeId::Uri),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::node_id::NodeId;

    #[test]
    fn an_iri_and_a_blank_node_are_both_node_identifiers() {
        assert!(matches!(NodeId::from("urn:ngsi-ld:Building:1"), NodeId::Uri(_)));
        assert!(matches!(NodeId::from("_:b0"), NodeId::Blank(_)));
        pretty_assertions::assert_eq!(NodeId::from("_:b0").to_string(), "_:b0");
    }

    /// An NGSI-LD document has no base IRI, so expansion keeps a relative
    /// reference, and a text that is no IRI at all, as written.
    #[test]
    fn a_relative_reference_and_a_text_that_is_no_iri_are_kept_verbatim() {
        for text in ["abc", "opening hours", "", "_:"] {
            let id = NodeId::from(text);
            assert!(matches!(id, NodeId::Verbatim(_)), "{text}: {id:?}");
            pretty_assertions::assert_eq!(id.as_str(), text);
        }
    }
}
