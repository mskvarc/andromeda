//! Lifting a `q` comparison against a value read out of JSONB into a
//! comparable domain.

use crate::{
    declared_operand::{boolean_reading, number_reading, string_reading},
    temporal_operand::{declared_date, declared_date_time, declared_time},
};
use andromeda_ngsi_ld_lexicon::time::{date::NgsiLdDate, time_of_day::NgsiLdTime};
use andromeda_ngsi_ld_query::filter::scalar::Scalar;
use andromeda_postgresql_seaquery::{cast::cast, json_type::jsonb_whole_text, pg_type::PostgresType};
use sea_query::{Expr as SeaExpr, SimpleExpr};

/// Both sides of one document comparison, lifted into the domain the operand's
/// kind is ordered in.
///
/// A value read out of JSONB is JSON, so neither side can be compared until
/// both have been lifted the same way. Without that a numeric comparison
/// orders lexicographically and `9` exceeds `10`.
pub struct LiftedComparison {
    /// The stored value, read out of the document and lifted.
    pub stored: SimpleExpr,
    /// The bound operand, lifted the same way.
    pub operand: SimpleExpr,
}

/// Lift a stored JSONB value and one `q` operand into a common domain.
///
/// `stored` is a JSONB expression, `datatype` the expanded datatype IRI the
/// value is declared as (`NULL` when none), and the operand carries its own
/// bound value, so sea-query numbers both sides' placeholders when the
/// enclosing statement renders.
///
/// The domains, by operand kind:
///
/// * a number reads a JSON number or a JSON string declared as an XSD number,
///   and a boolean a JSON boolean or a JSON string declared as `xsd:boolean`,
///   each cast on both sides,
/// * a string or a URI reads only a JSON string its datatype gives no other
///   data type, as text,
/// * a `DateTime`, `Date` or `Time` compares only with a value declared as one
///   of its form (clause 4.9: no equality of data types, no match), a
///   `DateTime` as `timestamptz` bound as the instant itself, a `Time` in the
///   fixed-width encoding the operand is rendered in, a `Date` as its one
///   text, which already sorts chronologically.
///
/// Numbers bind directly as `NUMERIC` and `DateTime` values as `timestamptz`;
/// booleans bind as text and are cast in the statement.
///
/// A stored value of another JSON type than the operand's (the String `"3310"`
/// against the number `3310`, the number `3310` against the String `"3310"`,
/// an object against a boolean) lifts to `NULL` rather than to its spelling, so
/// the comparison is unknown for that one value: clause 4.9 makes a target of
/// another data type unequal to the operand, and never makes the query fail
/// over it.
#[must_use]
pub fn lift_comparison(scalar: &Scalar, stored: SimpleExpr, datatype: SimpleExpr) -> LiftedComparison {
    match scalar {
        Scalar::Number(number) => LiftedComparison {
            stored: number_reading(stored, datatype),
            // SeaQuery owns every AST bind, so the exact decimal must be cloned
            // from the borrowed query operand.
            operand: SeaExpr::val(number.decimal().clone()),
        },
        Scalar::Bool(_) => LiftedComparison {
            stored: boolean_reading(stored, datatype),
            operand: cast(SeaExpr::val(scalar.literal_text()), PostgresType::Boolean),
        },
        Scalar::Str(_) | Scalar::Uri(_) => LiftedComparison {
            stored: string_reading(stored, datatype),
            operand: SeaExpr::val(scalar.literal_text()),
        },
        Scalar::DateTime(instant) => LiftedComparison {
            stored: declared_date_time(jsonb_whole_text(stored), datatype),
            operand: SeaExpr::val(*instant),
        },
        Scalar::Date(date) => LiftedComparison {
            stored: declared_date(jsonb_whole_text(stored), datatype),
            operand: SeaExpr::val(NgsiLdDate::from(*date).to_string()),
        },
        Scalar::Time(time) => LiftedComparison {
            stored: declared_time(jsonb_whole_text(stored), datatype),
            operand: SeaExpr::val(NgsiLdTime::normalise(*time).to_string()),
        },
    }
}

#[cfg(test)]
mod tests {
    use crate::jsonb::lift_comparison;
    use andromeda_ngsi_ld_query::filter::scalar::{Scalar, parse_date, parse_date_time, parse_time};
    use sea_query::{Alias, Expr as SeaExpr, PostgresQueryBuilder, Query, SimpleExpr, Value, Values};

    /// A plain column stands in for the JSONB value real callers lift.
    fn stored() -> SimpleExpr {
        SeaExpr::col(Alias::new("stored"))
    }

    /// The column standing in for the declared datatype.
    fn datatype() -> SimpleExpr {
        SeaExpr::col(Alias::new("datatype"))
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
        let lifted = lift_comparison(&Scalar::Number(9.into()), stored(), datatype());
        assert!(render(lifted.stored).contains("::numeric"), "the stored side casts to numeric");
        assert!(!render(lifted.operand.clone()).contains("::"), "the operand is already NUMERIC");
        assert_eq!(bound_value(lifted.operand), "9", "the operand binds NUMERIC, not a float");
    }

    /// Clause 4.9 makes a target of another data type unequal to the operand,
    /// so the String `"4"` is no number: only a JSON number, or a JSON string
    /// declared as an XSD number, reaches the cast.
    #[test]
    fn a_number_lift_reads_a_json_number_or_a_string_declared_as_one() {
        let sql = render(lift_comparison(&Scalar::Number(4.into()), stored(), datatype()).stored);
        assert!(
            sql.contains(r#"CASE WHEN (jsonb_typeof("stored") = ('number')) THEN ("stored" #>> ('{}'))::numeric WHEN (jsonb_typeof("stored") = ('string') AND "datatype" IN ("#),
            "{sql}"
        );
    }

    #[test]
    fn a_boolean_lift_reads_a_json_boolean_or_a_string_declared_as_one() {
        let sql = render(lift_comparison(&Scalar::Bool(true), stored(), datatype()).stored);
        assert!(
            sql.contains(r#"CASE WHEN (jsonb_typeof("stored") = ('boolean')) THEN ("stored" #>> ('{}'))::boolean WHEN (jsonb_typeof("stored") = ('string') AND "datatype" = ('http://www.w3.org/2001/XMLSchema#boolean')"#),
            "{sql}"
        );
    }

    #[test]
    fn a_boolean_is_lifted_to_boolean_on_both_sides() {
        let lifted = lift_comparison(&Scalar::Bool(true), stored(), datatype());
        assert!(render(lifted.stored).contains("::boolean"));
        assert!(render(lifted.operand.clone()).contains("::boolean"));
        assert_eq!(bound_value(lifted.operand), "true");
    }

    /// Clause 4.9 makes a target of another data type unequal to the operand,
    /// so the number `3310` is not the String `"3310"` its spelling reads as,
    /// and neither is a JSON string declared as a number.
    #[test]
    fn a_string_operand_reads_only_a_json_string_declared_as_no_other_data_type() {
        let lifted = lift_comparison(&Scalar::Str("red".to_string()), stored(), datatype());
        let sql = render(lifted.stored);
        assert!(
            sql.contains(r#"CASE WHEN (jsonb_typeof("stored") = ('string') AND (NOT COALESCE("datatype" IN ("#),
            "{sql}"
        );
        assert!(sql.contains(r#"THEN ("stored" #>> ('{}'))::text END"#), "{sql}");
        assert!(!render(lifted.operand.clone()).contains("::"), "the operand is not cast");
        assert_eq!(bound_value(lifted.operand), "red");
    }

    /// A declared `DateTime` is an instant, which text order does not order.
    #[test]
    fn a_date_time_is_compared_as_a_timestamp_not_as_text() {
        let instant = parse_date_time("2024-01-01T00:00:00Z").expect("valid date-time");
        let lifted = lift_comparison(&Scalar::DateTime(instant), stored(), datatype());
        let stored_sql = render(lifted.stored);
        assert!(stored_sql.contains("::timestamptz"), "{stored_sql}");
        assert!(stored_sql.contains("CASE WHEN "), "the cast must be guarded: {stored_sql}");
    }

    /// Truncating to whole seconds would make `>` admit an instant inside the
    /// truncated second that the client's operand excludes.
    #[test]
    fn a_date_time_operand_binds_as_the_instant_it_names() {
        let instant = parse_date_time("2024-01-01T00:00:00.123456Z").expect("valid date-time");
        let lifted = lift_comparison(&Scalar::DateTime(instant), stored(), datatype());
        let (_, Values(values)) = Query::select().expr(lifted.operand).to_owned().build(PostgresQueryBuilder);
        assert_eq!(values, vec![Value::ChronoDateTimeUtc(Some(instant))]);
    }

    #[test]
    fn a_time_is_brought_to_the_fixed_width_encoding_on_both_sides() {
        let time = parse_time("12:00:00,5Z").expect("the comma compatibility form");
        let lifted = lift_comparison(&Scalar::Time(time), stored(), datatype());
        assert!(render(lifted.stored).contains("AT TIME ZONE"), "the stored side normalises to UTC");
        assert_eq!(bound_value(lifted.operand), "12:00:00.500000Z");
    }

    #[test]
    fn a_date_is_compared_as_text_because_its_one_spelling_already_sorts_chronologically() {
        let date = parse_date("2024-01-01").expect("valid full-date");
        let lifted = lift_comparison(&Scalar::Date(date), stored(), datatype());
        let stored_sql = render(lifted.stored);
        assert!(!stored_sql.contains("::"), "the stored side is not cast: {stored_sql}");
        assert!(stored_sql.contains(r#""stored" #>> ('{}')"#), "the value is read as its text: {stored_sql}");
        assert!(stored_sql.contains("'https://uri.etsi.org/ngsi-ld/Date'"), "only a declared Date: {stored_sql}");
        assert_eq!(bound_value(lifted.operand), "2024-01-01");
    }
}
