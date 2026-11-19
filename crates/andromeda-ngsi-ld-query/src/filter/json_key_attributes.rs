//! The attributes a request declared to hold raw JSON.

use andromeda_ngsi_ld_lexicon::attribute_iri::AttributeIri;
use derive_more::From;
use std::collections::HashSet;

/// The expanded names of the attributes whose values are uninterpretable as
/// JSON-LD (clause 4.9 `jsonKeys`, Table 5.2.12-1).
///
/// A `q` filter addresses inside a value with the `[...]` path of clause 4.9,
/// and those member names are normally terms of the request `@context`: the
/// JSON-LD expansion of a structured Property value turns its members into
/// IRI-keyed nodes, so `address[city]` addresses the expansion of `city`.
/// Clause 4.9 EXAMPLE 11 is the exception this type carries: with
/// `?q=parkingTickets[value]=="Overstay 60 minutes"&jsonKeys=parkingTickets`,
/// "the target value raw is JSON, and is not expanded to ngsi-ld:hasValue using
/// the core @context". The member names under an opted-in attribute are
/// therefore literal object keys and are left exactly as the client wrote them.
///
/// The default is the empty set, which is what every filter that carries no
/// such declaration is expanded under.
#[derive(Debug, Default, Clone, PartialEq, Eq, From)]
pub struct JsonKeyAttributes(HashSet<AttributeIri>);

impl JsonKeyAttributes {
    /// Whether an expanded attribute name was declared to hold raw JSON.
    #[must_use]
    pub fn contains(&self, name: &AttributeIri) -> bool {
        self.0.contains(name)
    }

    /// Whether no attribute was declared to hold raw JSON.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl FromIterator<AttributeIri> for JsonKeyAttributes {
    fn from_iter<I: IntoIterator<Item = AttributeIri>>(names: I) -> JsonKeyAttributes {
        JsonKeyAttributes(names.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use crate::filter::json_key_attributes::JsonKeyAttributes;
    use andromeda_ngsi_ld_lexicon::attribute_iri::AttributeIri;

    fn name(raw: &str) -> AttributeIri {
        raw.parse().expect("an absolute IRI")
    }

    #[test]
    fn a_filter_that_declared_nothing_opts_no_attribute_in() {
        let declared = JsonKeyAttributes::default();
        assert!(declared.is_empty());
        assert!(!declared.contains(&name("https://example.com/parkingTickets")));
    }

    #[test]
    fn only_the_declared_attributes_are_recognised() {
        let declared: JsonKeyAttributes = [name("https://example.com/parkingTickets")].into_iter().collect();
        assert!(declared.contains(&name("https://example.com/parkingTickets")));
        assert!(!declared.contains(&name("https://example.com/address")));
        assert!(!declared.is_empty());
    }
}
