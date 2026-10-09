//! The NGSI-LD-aware JSONB constructions: lifting a `q` comparison into a
//! comparable domain, and building the jsonpath that reaches a stored value.

use crate::temporal_operand::{stored_date_time, stored_time};
use andromeda_ngsi_ld_lexicon::{ngsi_ld_date::NgsiLdDate, ngsi_ld_time::NgsiLdTime};
use andromeda_ngsi_ld_query::filter::scalar::Scalar;
use andromeda_ngsi_ld_vocabulary::expanded::properties::{CREATED_AT, DELETED_AT, EXPIRES_AT, JSON, MODIFIED_AT, OBJECT, OBSERVED_AT, UNIT_CODE};
use andromeda_postgresql_seaquery::{
    cast::{cast, cast_compound},
    jsonb::escape_jsonpath_key,
    pg_type::PostgresType,
};
use chrono::SecondsFormat;
use sea_query::{Expr as SeaExpr, SimpleExpr};
use std::iter::once;

/// Both sides of one document comparison, lifted into the domain the operand's
/// kind is ordered in.
///
/// A value read out of JSONB with `#>>` arrives as text, so neither side can be
/// compared until both have been lifted the same way. Without that a numeric
/// comparison orders lexicographically and `9` exceeds `10`.
pub struct LiftedComparison {
    /// The stored value, read out of the document and lifted.
    pub stored: SimpleExpr,
    /// The bound operand, lifted the same way.
    pub operand: SimpleExpr,
}

/// Lift a stored JSONB text expression and one `q` operand into a common domain.
///
/// `stored_text` is an expression already yielding `text` (the `… #>> '{}'`
/// form every document reader builds), and the operand carries its own bound
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
/// document member holding text of another shape yields `NULL` and simply does not
/// match, instead of aborting the statement.
///
/// Numbers bind directly as `NUMERIC`; booleans and `DateTime` values bind as
/// text and are cast in the statement.
#[must_use]
pub fn lift_comparison(scalar: &Scalar, stored_text: SimpleExpr) -> LiftedComparison {
    match scalar {
        Scalar::Number(number) => LiftedComparison {
            stored: cast_compound(stored_text, PostgresType::Numeric),
            // SeaQuery owns every AST bind, so the exact decimal must be cloned
            // from the borrowed query operand.
            operand: SeaExpr::val(number.decimal().clone()),
        },
        Scalar::Bool(_) => LiftedComparison {
            stored: cast_compound(stored_text, PostgresType::Boolean),
            operand: cast(SeaExpr::val(scalar.literal_text()), PostgresType::Boolean),
        },
        Scalar::Str(_) | Scalar::Uri(_) => LiftedComparison {
            stored: stored_text,
            operand: SeaExpr::val(scalar.literal_text()),
        },
        Scalar::DateTime(instant) => LiftedComparison {
            stored: stored_date_time(stored_text),
            operand: cast(SeaExpr::val(instant.to_rfc3339_opts(SecondsFormat::Micros, true)), PostgresType::Timestamptz),
        },
        Scalar::Date(date) => LiftedComparison {
            stored: stored_text,
            operand: SeaExpr::val(NgsiLdDate::from(*date).to_string()),
        },
        Scalar::Time(time) => LiftedComparison {
            stored: stored_time(stored_text),
            // A time of day with no canonical form is one no write path could
            // have stored; binding text nothing can equal is the non-match the
            // normalised stored column would produce anyway.
            operand: SeaExpr::val(NgsiLdTime::normalise(*time).map_or_else(|_| String::new(), |canonical| canonical.to_string())),
        },
    }
}

/// The jsonpath reaching every instance of the attribute element a path
/// addresses inside the expanded document: `$[0]."<attr>"[*]."<sub>"[*]…`.
///
/// Clause 4.9: "If a Property has multiple instances (identified by its
/// respective datasetId), and no datasetId is explicitly addressed, the target
/// value shall be any Value of such instances". The `q` grammar addresses no
/// datasetId, so every hop, the attribute's and each sub-attribute's, walks
/// every instance, and a term holds when one of the instances reached passes.
#[must_use]
pub fn attribute_instances_jsonpath(attr_iri: &str, sub_attrs: &[&str]) -> String {
    let mut jsonpath = String::from("$[0]");
    for name in once(attr_iri).chain(sub_attrs.iter().copied()) {
        jsonpath.push_str(".\"");
        jsonpath.push_str(&escape_jsonpath_key(name));
        jsonpath.push_str("\"[*]");
    }
    jsonpath
}

/// The jsonpath reaching the value of one attribute instance, relative to that
/// instance node: `$."<terminal>"[0]."<member>"[*]…."@value"`.
///
/// `element_iri` names the element the instance is an instance of: a system
/// sub-attribute (clause 4.9: `observedAt`, `unitCode`, and the clause 4.8
/// timestamps) is stored as a bare value node `{"@type":…,"@value":…}` with no
/// `hasValue`/`hasObject` carrier, so a path ending on one reaches its
/// `@value` with no terminal hop. Otherwise `terminal_iri` names the value
/// carrier, `hasValue` for a Property's value and `hasObject` for a
/// Relationship's object, and switches the final key between `@value` and
/// `@id`. `value_path` walks the members of a compound value.
///
/// The member hops come *before* the `@value` selector, not after it. JSON-LD
/// expansion of a structured Property value produces a node whose members are
/// themselves IRI-keyed arrays of value nodes (`hasValue[0].<city IRI>[0].@value`)
/// rather than one `@value` holding a raw JSON object.
#[must_use]
pub fn instance_value_jsonpath(element_iri: &str, value_path: &[&str], terminal_iri: &str) -> String {
    if value_path.is_empty() && is_system_value_node_iri(element_iri) {
        return String::from("$.\"@value\"");
    }
    let mut jsonpath = String::from("$.\"");
    jsonpath.push_str(&escape_jsonpath_key(terminal_iri));
    jsonpath.push_str("\"[0]");
    for member in value_path {
        jsonpath.push_str(".\"");
        jsonpath.push_str(&escape_jsonpath_key(member));
        jsonpath.push_str("\"[*]");
    }
    // A Relationship's object is keyed by `@id`, a Property's value by `@value`.
    // A member reached through a compound value is always a value node, whatever
    // carrier the attribute itself used, so only the unhopped path can be `@id`.
    let value_key = if value_path.is_empty() && terminal_iri == OBJECT.as_str() {
        "@id"
    } else {
        "@value"
    };
    jsonpath.push_str(".\"");
    jsonpath.push_str(value_key);
    jsonpath.push('"');
    jsonpath
}

/// The jsonpath reaching one member of a `JsonProperty` instance's value,
/// relative to that instance node: `$."<hasJSON>"[0]."@value"."<member>"…`.
///
/// A `JsonProperty` value is one `@json` literal (clause 4.5.24), so JSON-LD
/// expansion leaves its object whole under a single `@value`. Its members keep
/// the keys the writer chose and hold plain JSON, so each member hop is a plain
/// key and the path ends at the member itself rather than at a value node.
#[must_use]
pub fn instance_json_literal_member_jsonpath(value_path: &[&str]) -> String {
    let mut jsonpath = String::from("$.\"");
    jsonpath.push_str(&escape_jsonpath_key(JSON.as_str()));
    jsonpath.push_str("\"[0].\"@value\"");
    for member in value_path {
        jsonpath.push_str(".\"");
        jsonpath.push_str(&escape_jsonpath_key(member));
        jsonpath.push('"');
    }
    jsonpath
}

/// Whether an expanded IRI names a system sub-attribute stored as a bare value
/// node rather than as an attribute with a `hasValue`/`hasObject` carrier.
///
/// These are the members the entity layer injects and the client's
/// own `observedAt`, `unitCode` and `expiresAt`: each is written as
/// `[{"@type":…,"@value":…}]` (clauses 4.5.1, 4.8 and 4.5.2), so a clause 4.9 `q`
/// over one reaches its value at `@value` directly. The set is enumerated
/// explicitly: a term outside it is a nested attribute whose value still lives
/// behind a carrier.
fn is_system_value_node_iri(iri: &str) -> bool {
    iri == OBSERVED_AT.as_str()
        || iri == UNIT_CODE.as_str()
        || iri == CREATED_AT.as_str()
        || iri == MODIFIED_AT.as_str()
        || iri == DELETED_AT.as_str()
        || iri == EXPIRES_AT.as_str()
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_query::filter::scalar::{parse_date, parse_date_time, parse_time};
    use sea_query::{Alias, PostgresQueryBuilder, Query, Value, Values};

    /// A plain column stands in for the `#>> '{}'` extraction real
    /// callers lift.
    fn stored() -> SimpleExpr {
        SeaExpr::col(Alias::new("stored"))
    }

    /// Render one lifted operand to its SQL text alone.
    fn render(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    /// The one text value an operand binds.
    fn bound_value(expr: SimpleExpr) -> String {
        let (_, Values(values)) = Query::select().expr(expr).to_owned().build(PostgresQueryBuilder);
        match values.into_iter().next().expect("one bound value") {
            Value::String(Some(text)) => text,
            Value::BigDecimal(Some(value)) => value.to_string(),
            other @ (Value::Bool(_)
            | Value::TinyInt(_)
            | Value::SmallInt(_)
            | Value::Int(_)
            | Value::BigInt(_)
            | Value::TinyUnsigned(_)
            | Value::SmallUnsigned(_)
            | Value::Unsigned(_)
            | Value::BigUnsigned(_)
            | Value::Float(_)
            | Value::Double(_)
            | Value::BigDecimal(None)
            | Value::Enum(_)
            | Value::Char(_)
            | Value::Bytes(_)
            | Value::Json(_)
            | Value::ChronoDate(_)
            | Value::ChronoTime(_)
            | Value::ChronoDateTime(_)
            | Value::ChronoDateTimeUtc(_)
            | Value::ChronoDateTimeLocal(_)
            | Value::ChronoDateTimeWithTimeZone(_)
            | Value::Uuid(_)
            | Value::Array(..)) => panic!("a lifted operand binds text: {other:?}"),
            Value::String(None) => panic!("a lifted operand binds an absent value"),
        }
    }

    #[test]
    fn a_number_is_lifted_to_numeric_on_both_sides_so_it_is_not_ordered_as_text() {
        let lifted = lift_comparison(&Scalar::Number(9.into()), stored());
        assert!(render(lifted.stored).contains("::numeric"), "the stored side casts to numeric");
        assert!(!render(lifted.operand.clone()).contains("::"), "the operand is already NUMERIC");
        assert_eq!(bound_value(lifted.operand), "9", "the operand binds NUMERIC, not a float");
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
        // Clause 4.6.3 forbids a zone offset on a Time, so the spellings that
        // still need normalising are the fractional ones: a comma separator and
        // a short fraction both have to reach the fixed-width stored form.
        let time = parse_time("12:00:00,5Z").expect("the comma compatibility form");
        let lifted = lift_comparison(&Scalar::Time(time), stored());
        assert!(render(lifted.stored).contains("AT TIME ZONE"), "the stored side normalises to UTC");
        assert_eq!(bound_value(lifted.operand), "12:00:00.500000Z");
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

    /// A `JsonProperty` value is one `@json` literal, so its members are plain
    /// keys below that literal's `@value` and the path ends at the member.
    #[test]
    fn a_json_literal_member_path_hops_plain_keys_below_the_json_literal() {
        pretty_assertions::assert_eq!(
            instance_json_literal_member_jsonpath(&["value", "id"]),
            "$.\"https://uri.etsi.org/ngsi-ld/hasJSON\"[0].\"@value\".\"value\".\"id\""
        );
    }

    #[test]
    fn a_plain_attribute_value_path_selects_the_value_node_directly() {
        pretty_assertions::assert_eq!(
            instance_value_jsonpath("https://example.com/speed", &[], "https://uri.etsi.org/ngsi-ld/hasValue"),
            "$.\"https://uri.etsi.org/ngsi-ld/hasValue\"[0].\"@value\""
        );
    }

    #[test]
    fn a_relationship_object_path_terminates_at_the_identifier_key() {
        pretty_assertions::assert_eq!(
            instance_value_jsonpath("https://example.com/owner", &[], OBJECT.as_str()),
            "$.\"https://uri.etsi.org/ngsi-ld/hasObject\"[0].\"@id\""
        );
    }

    /// JSON-LD expansion of a
    /// structured Property value produces IRI-keyed member nodes hanging off
    /// `hasValue`, so the member hop precedes the `@value` selector rather than
    /// indexing into it.
    #[test]
    fn a_compound_member_is_hopped_before_the_value_selector_not_after_it() {
        pretty_assertions::assert_eq!(
            instance_value_jsonpath("https://example.com/address", &[CITY], "https://uri.etsi.org/ngsi-ld/hasValue"),
            format!("$.\"https://uri.etsi.org/ngsi-ld/hasValue\"[0].\"{CITY}\"[*].\"@value\"")
        );
    }

    #[test]
    fn nested_compound_members_hop_once_each_in_order() {
        let particulate = "https://uri.etsi.org/ngsi-ld/default-context/particulate";
        pretty_assertions::assert_eq!(
            instance_value_jsonpath("https://example.com/rawdata", &[CITY, particulate], "https://uri.etsi.org/ngsi-ld/hasValue"),
            format!("$.\"https://uri.etsi.org/ngsi-ld/hasValue\"[0].\"{CITY}\"[*].\"{particulate}\"[*].\"@value\"")
        );
    }

    /// A member reached through a compound value is a value node whatever
    /// carrier the attribute itself used, so the `hasObject` terminal does not
    /// drag `@id` down onto it.
    #[test]
    fn a_member_below_a_relationship_object_still_terminates_at_the_value_key() {
        pretty_assertions::assert_eq!(
            instance_value_jsonpath("https://example.com/owner", &[CITY], OBJECT.as_str()),
            format!("$.\"https://uri.etsi.org/ngsi-ld/hasObject\"[0].\"{CITY}\"[*].\"@value\"")
        );
    }

    /// Clause 4.9: a `q` on a system sub-attribute reaches its value directly,
    /// because it is stored as a bare value node with no `hasValue` carrier, so
    /// `q=temperature.observedAt` must not insert a `hasValue` hop.
    #[test]
    fn a_system_sub_attribute_terminates_at_its_value_with_no_carrier_hop() {
        for system in [
            OBSERVED_AT.as_str(),
            UNIT_CODE.as_str(),
            CREATED_AT.as_str(),
            MODIFIED_AT.as_str(),
            DELETED_AT.as_str(),
            EXPIRES_AT.as_str(),
        ] {
            pretty_assertions::assert_eq!(
                instance_value_jsonpath(system, &[], "https://uri.etsi.org/ngsi-ld/hasValue"),
                "$.\"@value\"",
                "{system} must reach @value with no hasValue hop"
            );
        }
    }

    /// A user-defined sub-attribute is not a system value
    /// node, so its value still lives behind the `hasValue` carrier.
    #[test]
    fn a_user_defined_sub_attribute_still_hops_through_its_carrier() {
        let accuracy = "https://uri.etsi.org/ngsi-ld/default-context/accuracy";
        pretty_assertions::assert_eq!(
            instance_value_jsonpath(accuracy, &[], "https://uri.etsi.org/ngsi-ld/hasValue"),
            "$.\"https://uri.etsi.org/ngsi-ld/hasValue\"[0].\"@value\""
        );
    }

    /// Clause 4.9 makes the target value any Value of the instances when no
    /// datasetId is addressed, so every hop walks every instance.
    #[test]
    fn every_attribute_and_sub_attribute_hop_walks_every_instance() {
        let accuracy = "https://uri.etsi.org/ngsi-ld/default-context/accuracy";
        pretty_assertions::assert_eq!(
            attribute_instances_jsonpath("https://example.com/temperature", &[accuracy]),
            format!("$[0].\"https://example.com/temperature\"[*].\"{accuracy}\"[*]")
        );
    }
}
