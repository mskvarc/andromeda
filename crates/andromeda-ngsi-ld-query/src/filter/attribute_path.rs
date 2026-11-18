//! The clause 4.9 `Attribute` of a Query Term: a value path, or a hop across a Relationship into the entity it names.

use crate::filter::value_path::ValuePath;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;

/// The attribute path a query term compares along (clause 4.9 `Attribute`).
///
/// Clause 4.9 gives an attribute reference three axes. Two of them stay inside
/// one entity and are carried by [`ValuePath`]. The third (the
/// `LinkedEntityRelation` production, written `rel{…}`) crosses a Relationship
/// into the entity it names, and may be chained. This enum is that distinction:
/// an attribute path is either a local value path, or one hop followed by an
/// attribute path that is itself either local or a further hop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttributePath {
    /// A value path addressing an attribute of the entity under evaluation.
    Local(ValuePath),
    /// A hop across a Relationship into the entity it links to.
    Linked {
        /// The Relationship the hop descends through.
        relationship: AttributeName,
        /// Entity types the linked entity is constrained to, empty when the hop
        /// carried no guard.
        ///
        /// Clause 4.9 writes the production as
        /// `*1(EntityType 1*("," EntityType) ":")` and its EXAMPLE 14 with one
        /// type, so a guard needs one or more comma-separated types, closed by
        /// a colon. A prefixed attribute name inside the braces is written
        /// quoted, because a bare `prefix:name` reads as a guard type followed
        /// by an attribute.
        guard_types: Vec<AttributeName>,
        /// The attribute path evaluated against the linked entity.
        inner: Box<AttributePath>,
    },
}

impl AttributePath {
    /// The value path this attribute path terminates at, the attribute whose
    /// value the query term ultimately compares.
    ///
    /// For a hop chain that is the innermost segment, which is the attribute a
    /// value-level concern such as `expandValues` or `jsonKeys` (clause 6.4.3.2)
    /// names: those parameters describe the values being compared, and after a
    /// hop the compared values belong to the linked entity's attribute.
    #[must_use]
    pub fn terminal(&self) -> &ValuePath {
        match self {
            AttributePath::Local(local) => local,
            AttributePath::Linked { inner, .. } => inner.terminal(),
        }
    }

    /// Whether this attribute path crosses into a linked entity.
    ///
    /// Testing the outermost segment is enough to answer that for the whole
    /// path: a hop wraps everything after it, so an [`AttributePath::Local`] can
    /// never have one nested inside.
    #[must_use]
    pub const fn is_linked(&self) -> bool {
        matches!(self, AttributePath::Linked { .. })
    }

    /// How many Relationship hops this attribute path crosses before it lands
    /// on a local attribute.
    ///
    /// Zero for a wholly local path, one for `rel{attr}`, two for
    /// `rel{rel2{attr}}`, and so on. Clause 5.7.2.4 measures a filter against
    /// `joinLevel` by exactly this count, so the number the traversal is bounded
    /// by and the number the rule checks are the same one.
    #[must_use]
    pub fn linked_depth(&self) -> u32 {
        match self {
            AttributePath::Local(_) => 0,
            AttributePath::Linked { inner, .. } => 1 + inner.linked_depth(),
        }
    }

    /// The value path this attribute path terminates at, mutably.
    pub fn terminal_mut(&mut self) -> &mut ValuePath {
        match self {
            AttributePath::Local(local) => local,
            AttributePath::Linked { inner, .. } => inner.terminal_mut(),
        }
    }
}
