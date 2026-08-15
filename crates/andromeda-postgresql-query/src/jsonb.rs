//! The NGSI-LD-aware JSONB constructions: lifting a `q` comparison into a
//! comparable domain, and building the jsonpath that reaches a stored value.

use crate::temporal_operand::{stored_date_time, stored_time};
use andromeda_ngsi_ld_lexicon::{ngsi_ld_date::NgsiLdDate, ngsi_ld_time::NgsiLdTime};
use andromeda_ngsi_ld_query::filter::scalar::Scalar;
use andromeda_ngsi_ld_vocabulary::expanded::properties::OBJECT;
use andromeda_postgresql_seaquery::{
    cast::{cast, cast_compound},
    jsonb::escape_jsonpath_key,
    pg_type::PostgresType,
};
use chrono::SecondsFormat;
use sea_query::{Expr as SeaExpr, SimpleExpr};

/// Both sides of one payload comparison, lifted into the domain the operand's
/// kind is ordered in.
///
/// A value read out of JSONB with `#>>` arrives as text, so neither side can be
/// compared until both have been lifted the same way. Without that a numeric
/// comparison orders lexicographically and `9` exceeds `10`.
pub struct LiftedComparison {
    /// The stored value, read out of the payload and lifted.
    pub stored: SimpleExpr,
    /// The bound operand, lifted the same way.
    pub operand: SimpleExpr,
}

/// Lift a stored JSONB text expression and one `q` operand into a common domain.
///
/// `stored_text` is an expression already yielding `text` — the `… #>> '{}'`
/// form every payload reader builds — and the operand carries its own bound
/// value, so sea-query numbers both sides' placeholders when the enclosing
/// statement renders.
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
///
/// A number, a boolean and a `DateTime` bind as decimal or RFC 3339 text and are
/// cast in the statement rather than encoded to a native type, so a value beyond
/// `f64`'s exact range survives the round trip exactly (clause 4.9 numeric
/// comparison).
pub fn lift_comparison(scalar: &Scalar, stored_text: SimpleExpr) -> LiftedComparison {
    match scalar {
        Scalar::Number(_) => LiftedComparison {
            stored: cast_compound(stored_text, PostgresType::Numeric),
            operand: cast(SeaExpr::val(scalar.literal_text()).into(), PostgresType::Numeric),
        },
        Scalar::Bool(_) => LiftedComparison {
            stored: cast_compound(stored_text, PostgresType::Boolean),
            operand: cast(SeaExpr::val(scalar.literal_text()).into(), PostgresType::Boolean),
        },
        Scalar::Str(_) => LiftedComparison {
            stored: stored_text,
            operand: SeaExpr::val(scalar.literal_text()).into(),
        },
        Scalar::DateTime(instant) => LiftedComparison {
            stored: stored_date_time(stored_text),
            operand: cast(
                SeaExpr::val(instant.to_rfc3339_opts(SecondsFormat::Micros, true)).into(),
                PostgresType::Timestamptz,
            ),
        },
        Scalar::Date(date) => LiftedComparison {
            stored: stored_text,
            operand: SeaExpr::val(NgsiLdDate::from(*date).to_string()).into(),
        },
        Scalar::Time(time) => LiftedComparison {
            stored: stored_time(stored_text),
            // A time of day with no canonical form is one no write path could
            // have stored; binding text nothing can equal is the non-match the
            // normalised stored column would produce anyway.
            operand: SeaExpr::val(NgsiLdTime::normalise(*time).map_or_else(|_| String::new(), |canonical| canonical.to_string())).into(),
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
    use sea_query::{Alias, PostgresQueryBuilder, Query, Value, Values};

    /// A plain column stands in for the `payload #>> '{}'` extraction real
    /// callers lift.
    fn stored() -> SimpleExpr {
        SeaExpr::col(Alias::new("stored")).into()
    }

    /// Render one lifted operand to its SQL text alone.
    fn render(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    /// The one value an operand binds, rendered as its text.
    fn bound_value(expr: SimpleExpr) -> String {
        let (_, Values(values)) = Query::select().expr(expr).to_owned().build(PostgresQueryBuilder);
        match values.into_iter().next().expect("one bound value") {
            Value::String(Some(text)) => *text,
            other => unreachable!("a lifted operand binds text: {other:?}"),
        }
    }

    #[test]
    fn a_number_is_lifted_to_numeric_on_both_sides_so_it_is_not_ordered_as_text() {
        let lifted = lift_comparison(&Scalar::Number(9.into()), stored());
        assert!(render(lifted.stored).contains("::numeric"), "the stored side casts to numeric");
        assert!(render(lifted.operand.clone()).contains("::numeric"), "the operand casts to numeric");
        assert_eq!(bound_value(lifted.operand), "9", "the operand binds decimal text, not a float");
    }

    #[test]
    fn a_boolean_is_lifted_to_boolean_on_both_sides() {
        let lifted = lift_comparison(&Scalar::Bool(true), stored());
        assert!(render(lifted.stored).contains("::boolean"));
        assert!(render(lifted.operand.clone()).contains("::boolean"));
        assert_eq!(bound_value(lifted.operand), "true");
    }

    #[test]
    fn a_string_operand_is_compared_as_the_text_it_already_is() {
        let lifted = lift_comparison(&Scalar::Str("red".to_string()), stored());
        assert!(!render(lifted.stored).contains("::"), "the stored side is not cast");
        assert!(!render(lifted.operand.clone()).contains("::"), "the operand is not cast");
        assert_eq!(bound_value(lifted.operand), "red");
    }

    #[test]
    fn a_date_time_is_compared_as_a_timestamp_not_as_text() {
        // Clause 4.6.3 admits any offset, so `2024-01-01T01:00:00+01:00` and
        // `2024-01-01T00:00:00Z` are one instant that text order separates.
        let instant = parse_date_time("2024-01-01T00:00:00Z").expect("valid date-time");
        let lifted = lift_comparison(&Scalar::DateTime(instant), stored());
        let stored_sql = render(lifted.stored);
        assert!(stored_sql.contains("::timestamptz"), "{stored_sql}");
        assert!(stored_sql.contains("CASE WHEN "), "the cast must be guarded: {stored_sql}");
        assert!(render(lifted.operand).contains("::timestamptz"), "the operand casts to timestamptz");
    }

    #[test]
    fn a_date_time_operand_keeps_its_sub_second_precision() {
        // Truncating to whole seconds would make `>` admit an instant inside the
        // truncated second that the client's operand excludes.
        let instant = parse_date_time("2024-01-01T00:00:00.123456Z").expect("valid date-time");
        let lifted = lift_comparison(&Scalar::DateTime(instant), stored());
        assert_eq!(bound_value(lifted.operand), "2024-01-01T00:00:00.123456Z");
    }

    #[test]
    fn a_time_is_normalised_on_both_sides_into_the_canonical_form() {
        let time = parse_time("13:00:00+01:00").expect("valid full-time");
        let lifted = lift_comparison(&Scalar::Time(time), stored());
        assert!(render(lifted.stored).contains("AT TIME ZONE"), "the stored side normalises to UTC");
        assert_eq!(bound_value(lifted.operand), "12:00:00.000000Z");
    }

    #[test]
    fn a_date_is_compared_as_text_because_its_canonical_form_already_sorts_chronologically() {
        let date = parse_date("2024-01-01").expect("valid full-date");
        let lifted = lift_comparison(&Scalar::Date(date), stored());
        assert!(!render(lifted.stored).contains("::"), "the stored side is not cast");
        assert_eq!(bound_value(lifted.operand), "2024-01-01");
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
