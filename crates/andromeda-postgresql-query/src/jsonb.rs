//! Lifting a `q` comparison against a value read out of JSONB into a
//! comparable domain.

use crate::temporal_operand::{stored_date_time, stored_time};
use andromeda_ngsi_ld_lexicon::{ngsi_ld_date::NgsiLdDate, ngsi_ld_time::NgsiLdTime};
use andromeda_ngsi_ld_query::filter::scalar::Scalar;
use andromeda_postgresql_seaquery::{
    cast::{cast, cast_compound},
    pg_type::PostgresType,
};
use chrono::SecondsFormat;
use sea_query::{Expr as SeaExpr, SimpleExpr};

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
}
