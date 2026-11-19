//! Why a `pick`, `omit` or `attrs` projection is refused (clause 4.21).

use crate::projection::{expanded_projection_term::ExpandedProjectionTerm, pick_synonym::PickSynonym};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, reference::NgsiLdReferenceError};
use thiserror::Error;

/// The clause 4.21 rule a projection broke.
#[derive(Debug, Error)]
pub enum ProjectionError {
    /// A `{` opened without a matching `}`, a `}` closed without a matching
    /// `{`, or content followed a complete term outside the grammar.
    #[error("projection: unbalanced brace at position {position}")]
    UnbalancedBrace {
        /// Zero-based byte offset of the offending brace or trailing content.
        position: usize,
    },
    /// A term position (list start, after a separator, or inside `{}`)
    /// produced no attribute name.
    #[error("projection: empty term at position {position}")]
    EmptyTerm {
        /// Zero-based byte offset where a name was expected.
        position: usize,
    },
    /// A `pick` or `omit` name was not usable as an attribute name (clause 5.2.4).
    #[error("invalid attribute name: {0}")]
    InvalidAttributeName(#[source] NgsiLdReferenceError),
    /// A projection keyword (`id`, `type`, `scope`) carried a clause 4.21
    /// `{...}` linked-entity sub-projection.
    ///
    /// `LinkedEntityTerm` only ever follows `AttrName` in the grammar; the
    /// keywords name structural members of the representation, not
    /// relationships, so they have no linked entity to project.
    #[error("projection: keyword '{keyword}' cannot carry a linked-entity term")]
    KeywordWithLinkedTerm {
        /// The offending keyword.
        keyword: AttributeName,
    },
    /// `pick` and `omit` name the same member of the same Entity once both are expanded against the request `@context`, so its pick selects nothing (clause 4.21).
    #[error("'{member}' is named in both pick and omit")]
    MemberPickedAndOmitted {
        /// The member both lists name: a clause 4.21 keyword, or an Attribute by its expanded name.
        member: ExpandedProjectionTerm,
    },
    /// The deprecated synonym for `pick` named an Entity member its operation does not admit. The refusal is decided on the expanded name, so a context alias of the Scope is refused as the Scope itself.
    #[error("'{term}' is an Entity member and is not allowed in {synonym}")]
    ReservedAttrsMember {
        /// The name as the request spelled it.
        term: AttributeName,
        /// The member the request wrote the name in.
        synonym: PickSynonym,
    },
}
