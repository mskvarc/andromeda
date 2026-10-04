//! How the members of a clause 4.9 trailing path are spelled.

use crate::filter::json_key_attributes::JsonKeyAttributes;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;

/// Whether the `[…]` members of a filter's attribute paths are terms of the
/// `@context` or literal JSON object keys.
///
/// Clause 4.9 evaluates a trailing path "as a `MemberExpression` of ECMA 262"
/// against the seed target value, so a member names whatever key that value
/// carries. An Entity's structured Property value is stored JSON-LD expanded,
/// which keys its members by IRI, so its members are terms; a raw JSON value
/// keeps the keys the writer chose, so its members are literal.
#[derive(Debug, Clone, Copy)]
pub enum TrailingMembers<'a> {
    /// The members of an Entity filter (`q`): terms, except under the
    /// attributes the request declared under `jsonKeys`, whose values are raw
    /// JSON (clause 4.9 EXAMPLE 11).
    Terms(&'a JsonKeyAttributes),
    /// The members of a Context Source filter (`csf`): literal keys always.
    /// Clause 4.9 targets "the name of a Context Source Property member of the
    /// `CSourceRegistration`", and a registration holds those values as the
    /// registrant wrote them, so no `@context` ever rewrote their keys.
    Verbatim,
}

impl TrailingMembers<'_> {
    /// Whether the members addressed under the expanded attribute `attribute`
    /// are terms to expand or compact.
    #[must_use]
    pub fn are_terms_under(self, attribute: &AttributeName) -> bool {
        match self {
            TrailingMembers::Terms(json_keys) => !json_keys.contains(attribute),
            TrailingMembers::Verbatim => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::filter::{json_key_attributes::JsonKeyAttributes, trailing_members::TrailingMembers};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;

    fn name(raw: &str) -> AttributeName {
        raw.parse().expect("valid attribute name")
    }

    #[test]
    fn entity_filter_members_are_terms_unless_json_keys_declared_the_attribute() {
        let declared: JsonKeyAttributes = [name("https://example.com/parkingTickets")].into_iter().collect();
        let members = TrailingMembers::Terms(&declared);
        assert!(members.are_terms_under(&name("https://example.com/address")));
        assert!(!members.are_terms_under(&name("https://example.com/parkingTickets")));
    }

    #[test]
    fn context_source_filter_members_are_never_terms() {
        assert!(!TrailingMembers::Verbatim.are_terms_under(&name("https://example.com/coverage")));
    }
}
