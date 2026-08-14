//! The NGSI-LD-aware JSONB constructions: lifting a `q` comparison into a
//! comparable domain, and building the jsonpath that reaches a stored value.

use crate::temporal_operand::{stored_date_time, stored_time};
use andromeda_ngsi_ld_lexicon::{ngsi_ld_date::NgsiLdDate, ngsi_ld_time::NgsiLdTime};
use andromeda_ngsi_ld_query::filter::scalar::Scalar;
use andromeda_ngsi_ld_vocabulary::expanded::properties::OBJECT;
use andromeda_postgresql_seaquery::jsonb::escape_jsonpath_key;
use chrono::SecondsFormat;

/// Both sides of one payload comparison, lifted into the domain the operand's
/// kind is ordered in.
///
/// A value read out of JSONB with `#>>` arrives as text, so neither side can be
/// compared until both have been lifted the same way. Without that a numeric
/// comparison orders lexicographically and `9` exceeds `10`.
pub struct ComparisonSql {
    /// The stored value, read out of the payload and lifted.
    pub stored: String,
    /// The bound operand, lifted the same way.
    pub operand: String,
    /// The literal text bound at the operand's placeholder.
    pub value: String,
}

/// Lift a stored JSONB text expression and one `q` operand into a common domain.
///
/// `stored_text` is a SQL expression already yielding `text` — the `… #>> '{}'`
/// form every payload reader builds — and `placeholder` is the bind marker the
/// operand travels in.
///
/// The domains, by operand kind:
///
/// * numbers and booleans cast on both sides,
/// * strings compare as text, which is what they are,
/// * a `DateTime` compares as `timestamptz`, so PostgreSQL resolves each side's
///   offset rather than the statement ordering two spellings of one instant by
///   their text. The operand is rendered at full microsecond precision, because
///   truncating it to whole seconds would make `>` admit an instant inside the
///   truncated second that the client's operand excludes,
/// * a `Time` compares as canonical text, with the stored value normalised to the
///   same fixed-width UTC form the operand is rendered in,
/// * a `Date` compares as text unlifted: its canonical `YYYY-MM-DD` form is
///   already fixed-width and zero-padded, so text order over it is chronological
///   order and a `::date` cast would buy nothing but a cast failure to guard.
///
/// Both temporal lifts are regex-guarded (see [`crate::temporal_operand`]): a
/// payload member holding text of another shape yields `NULL` and simply does not
/// match, instead of aborting the statement.
pub fn comparison_sql(scalar: &Scalar, stored_text: &str, placeholder: &str) -> ComparisonSql {
    match scalar {
        Scalar::Number(_) => ComparisonSql {
            stored: format!("({stored_text})::numeric"),
            operand: format!("{placeholder}::numeric"),
            value: scalar.literal_text(),
        },
        Scalar::Bool(_) => ComparisonSql {
            stored: format!("({stored_text})::boolean"),
            operand: format!("{placeholder}::boolean"),
            value: scalar.literal_text(),
        },
        Scalar::Str(_) => ComparisonSql {
            stored: format!("({stored_text})"),
            operand: placeholder.to_owned(),
            value: scalar.literal_text(),
        },
        Scalar::DateTime(instant) => ComparisonSql {
            stored: stored_date_time(stored_text),
            operand: format!("{placeholder}::timestamptz"),
            value: instant.to_rfc3339_opts(SecondsFormat::Micros, true),
        },
        Scalar::Date(date) => ComparisonSql {
            stored: format!("({stored_text})"),
            operand: placeholder.to_owned(),
            value: NgsiLdDate::from(*date).to_string(),
        },
        Scalar::Time(time) => ComparisonSql {
            stored: stored_time(stored_text),
            operand: placeholder.to_owned(),
            // A time of day with no canonical form is one no write path could
            // have stored; binding text nothing can equal is the non-match the
            // normalised stored column would produce anyway.
            value: NgsiLdTime::normalise(*time).map_or_else(|_| String::new(), |canonical| canonical.to_string()),
        },
    }
}

/// Build the jsonpath that reaches an attribute's value inside the expanded
/// payload: `$[0]."<attr>"[0]."<sub>"…[0]."<terminal>"[0]."<member>"[*]…."@value"`.
///
/// `terminal_iri` names the value carrier — `hasValue` for a Property's value,
/// `hasObject` for a Relationship's object — and switches the final key between
/// `@value` and `@id` accordingly. `sub_attrs` walks nested attributes and
/// `value_path` walks the members of a compound value; both are empty for the
/// common top-level `hasValue` read a geo-query or an `orderBy` term makes.
/// Every builder that reaches a stored attribute value shares this one
/// construction, so the payload's expanded shape is described in a single place.
///
/// The member hops come *before* the `@value` selector, not after it. JSON-LD
/// expansion of a structured Property value produces a node whose members are
/// themselves IRI-keyed arrays of value nodes — `hasValue[0].<city IRI>[0].@value`
/// — rather than one `@value` holding a raw JSON object. Selecting `@value`
/// first and then indexing into it addressed a member of a node that does not
/// exist, so every compound-value predicate resolved nothing.
pub fn attribute_value_jsonpath(attr_iri: &str, sub_attrs: &[&str], value_path: &[&str], terminal_iri: &str) -> String {
    let mut jsonpath = format!("$[0].\"{}\"", escape_jsonpath_key(attr_iri));
    for sub in sub_attrs {
        jsonpath.push_str(&format!("[0].\"{}\"", escape_jsonpath_key(sub)));
    }
    jsonpath.push_str(&format!("[0].\"{}\"[0]", escape_jsonpath_key(terminal_iri)));
    for member in value_path {
        jsonpath.push_str(&format!(".\"{}\"[*]", escape_jsonpath_key(member)));
    }
    // A Relationship's object is keyed by `@id`, a Property's value by `@value`.
    // A member reached through a compound value is always a value node, whatever
    // carrier the attribute itself used, so only the unhopped path can be `@id`.
    let value_key = if value_path.is_empty() && terminal_iri == OBJECT.as_str() {
        "@id"
    } else {
        "@value"
    };
    jsonpath.push_str(&format!(".\"{value_key}\""));
    jsonpath
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_query::filter::scalar::{parse_date, parse_date_time, parse_time};

    #[test]
    fn a_number_is_lifted_to_numeric_on_both_sides_so_it_is_not_ordered_as_text() {
        let lifted = comparison_sql(&Scalar::Number(9.into()), "stored", "$1");
        assert_eq!(lifted.stored, "(stored)::numeric");
        assert_eq!(lifted.operand, "$1::numeric");
        assert_eq!(lifted.value, "9");
    }

    #[test]
    fn a_boolean_is_lifted_to_boolean_on_both_sides() {
        let lifted = comparison_sql(&Scalar::Bool(true), "stored", "$1");
        assert_eq!(lifted.stored, "(stored)::boolean");
        assert_eq!(lifted.operand, "$1::boolean");
        assert_eq!(lifted.value, "true");
    }

    #[test]
    fn a_string_operand_is_compared_as_the_text_it_already_is() {
        let lifted = comparison_sql(&Scalar::Str("red".to_string()), "stored", "$1");
        assert_eq!(lifted.stored, "(stored)");
        assert_eq!(lifted.operand, "$1");
        assert_eq!(lifted.value, "red");
    }

    #[test]
    fn a_date_time_is_compared_as_a_timestamp_not_as_text() {
        // Clause 4.6.3 admits any offset, so `2024-01-01T01:00:00+01:00` and
        // `2024-01-01T00:00:00Z` are one instant that text order separates.
        let instant = parse_date_time("2024-01-01T00:00:00Z").expect("valid date-time");
        let lifted = comparison_sql(&Scalar::DateTime(instant), "stored", "$1");
        assert!(lifted.stored.contains("::timestamptz"), "{}", lifted.stored);
        assert!(lifted.stored.starts_with("(CASE WHEN "), "the cast must be guarded: {}", lifted.stored);
        assert_eq!(lifted.operand, "$1::timestamptz");
    }

    #[test]
    fn a_date_time_operand_keeps_its_sub_second_precision() {
        // Truncating to whole seconds would make `>` admit an instant inside the
        // truncated second that the client's operand excludes.
        let instant = parse_date_time("2024-01-01T00:00:00.123456Z").expect("valid date-time");
        let lifted = comparison_sql(&Scalar::DateTime(instant), "stored", "$1");
        assert_eq!(lifted.value, "2024-01-01T00:00:00.123456Z");
    }

    #[test]
    fn a_time_is_normalised_on_both_sides_into_the_canonical_form() {
        let time = parse_time("13:00:00+01:00").expect("valid full-time");
        let lifted = comparison_sql(&Scalar::Time(time), "stored", "$1");
        assert!(lifted.stored.contains("AT TIME ZONE 'UTC'"), "{}", lifted.stored);
        assert_eq!(lifted.operand, "$1");
        assert_eq!(lifted.value, "12:00:00.000000Z");
    }

    #[test]
    fn a_date_is_compared_as_text_because_its_canonical_form_already_sorts_chronologically() {
        let date = parse_date("2024-01-01").expect("valid full-date");
        let lifted = comparison_sql(&Scalar::Date(date), "stored", "$1");
        assert_eq!(lifted.stored, "(stored)");
        assert_eq!(lifted.value, "2024-01-01");
    }

    /// The IRI a compact compound-value member expands to under the core
    /// context's `@vocab`, which is what a request-expanded path carries.
    const CITY: &str = "https://uri.etsi.org/ngsi-ld/default-context/city";

    #[test]
    fn a_plain_attribute_value_path_selects_the_value_node_directly() {
        pretty_assertions::assert_eq!(
            attribute_value_jsonpath("https://example.com/speed", &[], &[], "https://uri.etsi.org/ngsi-ld/hasValue"),
            "$[0].\"https://example.com/speed\"[0].\"https://uri.etsi.org/ngsi-ld/hasValue\"[0].\"@value\""
        );
    }

    #[test]
    fn a_relationship_object_path_terminates_at_the_identifier_key() {
        pretty_assertions::assert_eq!(
            attribute_value_jsonpath("https://example.com/owner", &[], &[], OBJECT.as_str()),
            "$[0].\"https://example.com/owner\"[0].\"https://uri.etsi.org/ngsi-ld/hasObject\"[0].\"@id\""
        );
    }

    /// The defect this construction exists to fix: JSON-LD expansion of a
    /// structured Property value produces IRI-keyed member nodes hanging off
    /// `hasValue`, so the member hop precedes the `@value` selector rather than
    /// indexing into it.
    #[test]
    fn a_compound_member_is_hopped_before_the_value_selector_not_after_it() {
        pretty_assertions::assert_eq!(
            attribute_value_jsonpath("https://example.com/address", &[], &[CITY], "https://uri.etsi.org/ngsi-ld/hasValue"),
            format!("$[0].\"https://example.com/address\"[0].\"https://uri.etsi.org/ngsi-ld/hasValue\"[0].\"{CITY}\"[*].\"@value\"")
        );
    }

    #[test]
    fn nested_compound_members_hop_once_each_in_order() {
        let particulate = "https://uri.etsi.org/ngsi-ld/default-context/particulate";
        pretty_assertions::assert_eq!(
            attribute_value_jsonpath(
                "https://example.com/sensor",
                &["https://example.com/rawdata"],
                &[CITY, particulate],
                "https://uri.etsi.org/ngsi-ld/hasValue"
            ),
            format!(
                "$[0].\"https://example.com/sensor\"[0].\"https://example.com/rawdata\"[0].\"https://uri.etsi.org/ngsi-ld/hasValue\"[0]\
                 .\"{CITY}\"[*].\"{particulate}\"[*].\"@value\""
            )
        );
    }

    /// A member reached through a compound value is a value node whatever
    /// carrier the attribute itself used, so the `hasObject` terminal does not
    /// drag `@id` down onto it.
    #[test]
    fn a_member_below_a_relationship_object_still_terminates_at_the_value_key() {
        pretty_assertions::assert_eq!(
            attribute_value_jsonpath("https://example.com/owner", &[], &[CITY], OBJECT.as_str()),
            format!("$[0].\"https://example.com/owner\"[0].\"https://uri.etsi.org/ngsi-ld/hasObject\"[0].\"{CITY}\"[*].\"@value\"")
        );
    }
}
