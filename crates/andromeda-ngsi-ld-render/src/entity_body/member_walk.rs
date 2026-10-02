//! What the walk over an entity's attribute members applies on the way.

use crate::{entity_body::linked_entities::AttributeJoin, uri_decoding::UriDecoding};
use andromeda_http_headers::language::LanguageFilter;

/// The presentation-boundary rules the walk applies while it writes an
/// attribute member: URI decoding (everywhere), and clause 4.15 language
/// negotiation and the clause 4.5.23 inline join (top-level attributes only).
#[derive(Debug, Clone, Copy)]
pub(crate) struct MemberWalk<'a> {
    /// Whether Relationship targets are percent-decoded.
    pub(crate) decoding: UriDecoding,
    /// The clause 4.15 `lang` filter a top-level `LanguageProperty` is
    /// negotiated against, if the request sent one.
    pub(crate) language_filter: Option<&'a LanguageFilter>,
    /// The inline join a top-level Relationship nests its targets by, if the
    /// response asked for one.
    pub(crate) join: Option<AttributeJoin<'a>>,
}

impl<'a> MemberWalk<'a> {
    /// The walk over a top-level attribute member.
    pub(crate) const fn new(decoding: UriDecoding, language_filter: Option<&'a LanguageFilter>) -> MemberWalk<'a> {
        MemberWalk {
            decoding,
            language_filter,
            join: None,
        }
    }

    /// The same walk, nesting the targets of the member by `join`.
    pub(crate) const fn with_join(self, join: Option<AttributeJoin<'a>>) -> MemberWalk<'a> {
        MemberWalk {
            decoding: self.decoding,
            language_filter: self.language_filter,
            join,
        }
    }

    /// The walk over a sub-attribute, which clause 4.15 does not negotiate --
    /// the filter selects among the Attributes of the Entity, and a
    /// sub-attribute is written as it is -- and whose targets clause 4.5.23
    /// does not nest.
    pub(crate) const fn nested(self) -> MemberWalk<'a> {
        MemberWalk {
            decoding: self.decoding,
            language_filter: None,
            join: None,
        }
    }
}
