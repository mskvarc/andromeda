//! Which attribute names a compiled `q` filter references by dictionary key.

use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::filter::reading::filter_reading::FilterReading;

/// Attribute names the reading of a `q` filter reads by dictionary key.
///
/// The attribute of each term and every sub-attribute on its path are keyed:
/// a row and each element of its `sub_attributes` name their attribute by
/// key. A path ending on a member of an instance (`observedAt`, `datasetId`,
/// `valueType` and the rest) ends on a field, the term's terminal, and the
/// members of a compound value are navigated inside a `value_object`, so
/// neither is resolved. Each hop contributes the Relationship it descends
/// through as well, because the hop probes that Relationship's instances by
/// dictionary key.
pub fn referenced_attribute_names(reading: &FilterReading, out: &mut Vec<AttributeName>) {
    for term in reading.terms() {
        // clone: the caller owns the resolved list, which outlives the reading
        out.extend(term.hops().iter().map(|hop| AttributeName::from(hop.relationship().clone())));
        out.push(AttributeName::from(term.attribute().clone()));
        out.extend(term.sub_attributes().iter().cloned());
    }
}

#[cfg(test)]
mod tests {
    use crate::q_predicate::{attribute_names::referenced_attribute_names, lowered_filter::reading_of};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use pretty_assertions::assert_eq;

    fn collected(q: &str) -> Vec<String> {
        let mut names = Vec::new();
        referenced_attribute_names(&reading_of(q), &mut names);
        names.sort();
        names.iter().map(AttributeName::as_str).map(str::to_string).collect()
    }

    #[test]
    fn every_predicate_target_of_a_boolean_expression_is_collected() {
        assert_eq!(
            collected("\"https://example.com/a\">1;\"https://example.com/b\"<2|\"https://example.com/c\"==\"x\""),
            vec!["https://example.com/a", "https://example.com/b", "https://example.com/c"]
        );
    }

    /// A member of an instance is a field of it, so it has no dictionary row
    /// to resolve; a sub-attribute is an element keyed by its name.
    #[test]
    fn sub_attribute_segments_are_collected_and_member_segments_are_not() {
        assert_eq!(
            collected("\"https://example.com/temperature\".\"https://uri.etsi.org/ngsi-ld/observedAt\">=\"2024-01-01T00:00:00Z\""),
            vec!["https://example.com/temperature"]
        );
        assert_eq!(
            collected("\"https://example.com/temperature\".\"https://example.com/accuracy\".\"https://example.com/method\"==\"x\""),
            vec!["https://example.com/accuracy", "https://example.com/method", "https://example.com/temperature"]
        );
        assert_eq!(
            collected("\"https://example.com/feeds\".\"https://example.com/since\".\"https://uri.etsi.org/ngsi-ld/hasObjectType\"==\"x\""),
            vec!["https://example.com/feeds", "https://example.com/since"]
        );
    }

    #[test]
    fn every_relationship_a_traversal_descends_is_collected_with_the_terminal_attribute() {
        // Clause 4.9: each hop probes the instances of its
        // Relationship by dictionary key, exactly as the terminal comparison
        // does.
        assert_eq!(
            collected("\"https://example.com/outer\"{\"https://example.com/inner\"{\"https://example.com/zone\"}}==\"north\""),
            vec!["https://example.com/inner", "https://example.com/outer", "https://example.com/zone"]
        );
    }
}
