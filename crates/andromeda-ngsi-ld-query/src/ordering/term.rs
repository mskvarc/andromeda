//! Ordering terms, before and after context expansion (clause 4.23).

use crate::ordering::collation::IcuCollation;
use iri_rs::IriBuf;

/// A single parsed ordering term from the `orderBy` parameter.
///
/// `attr` is the raw request text, before it is classified and expanded into a
/// [`CompiledOrderTerm`]: at this point it is whatever the client wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderTerm {
    /// The attribute name or clause 4.23 keyword to sort by, as written.
    pub attr: String,
    /// The sort direction.
    pub direction: OrderDirection,
}

/// What an `orderBy` term sorts on (clause 4.23).
///
/// `id` and `type` name structural members of the entity representation, not
/// attributes, so they have no IRI to expand to and they are read from their
/// own columns rather than from the payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrderTarget {
    /// The entity identifier.
    Id,
    /// The entity types.
    Type,
    /// One attribute's value, named by its expanded IRI.
    Attribute(IriBuf),
}

/// A raw `orderBy` name, classified but not yet expanded.
///
/// Classification is separated from expansion so the clause 4.23 keyword set is
/// stated once, while the caller — which needs a JSON-LD lookup, and therefore
/// an `await`, only for the attribute arm — decides how the expansion happens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawOrderTarget<'a> {
    /// The `id` keyword.
    Id,
    /// The `type` keyword.
    Type,
    /// An attribute name, still compact.
    Attribute(&'a str),
}

impl<'a> RawOrderTarget<'a> {
    /// The `id` keyword of clause 4.23.
    pub const ID_KEYWORD: &'static str = "id";
    /// The `type` keyword of clause 4.23.
    pub const TYPE_KEYWORD: &'static str = "type";

    /// Classify a raw `orderBy` name.
    pub fn classify(raw: &'a str) -> RawOrderTarget<'a> {
        match raw {
            RawOrderTarget::ID_KEYWORD => RawOrderTarget::Id,
            RawOrderTarget::TYPE_KEYWORD => RawOrderTarget::Type,
            // `&str` is unbounded, so a catch-all is the only way to close this
            // match; anything that is not one of the two keywords is an
            // attribute name.
            other => RawOrderTarget::Attribute(other),
        }
    }
}

/// A single term in an `orderBy` clause after context expansion.
#[derive(Debug, Clone)]
pub struct CompiledOrderTerm {
    /// What the term sorts on.
    pub target: OrderTarget,
    /// The sort direction (ascending, descending, or distance-based).
    pub direction: OrderDirection,
    /// ICU collation identifier for string sorting (S4.23).
    pub collation: Option<IcuCollation>,
}

/// Sort direction for `orderBy` terms per NGSI-LD S4.23.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderDirection {
    /// Sort in ascending order.
    Asc,
    /// Sort in descending order.
    Desc,
    /// Sort by distance in ascending order from the geo query reference geometry (S4.23).
    DistAsc,
    /// Sort by distance in descending order from the geo query reference geometry (S4.23).
    DistDesc,
}

impl OrderDirection {
    /// Whether this is a distance-based ordering direction.
    pub const fn is_distance(&self) -> bool {
        matches!(self, OrderDirection::DistAsc | OrderDirection::DistDesc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_order_by_term_of_id_or_type_is_not_expanded() {
        // Clause 4.23: `id` and `type` name members of the representation, not
        // attributes, so no context lookup applies to them and neither reaches
        // the attribute arm that performs one.
        assert_eq!(RawOrderTarget::classify("id"), RawOrderTarget::Id);
        assert_eq!(RawOrderTarget::classify("type"), RawOrderTarget::Type);
    }

    #[test]
    fn any_other_order_by_term_is_classified_as_an_attribute() {
        assert_eq!(RawOrderTarget::classify("speed"), RawOrderTarget::Attribute("speed"));
    }
}
