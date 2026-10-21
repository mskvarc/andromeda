//! Lifting stored temporal text out of JSONB into the domain clause 4.9 orders
//! it in.
//!
//! A `DateTime` or `Time` read out of a JSONB document with `#>>` arrives as
//! the text the client wrote. Clause 4.6.3 admits more than one spelling of one
//! value (any fractional width, a comma for the decimal point), so text
//! comparison over it is not the temporal comparison clause 4.9 asks for. Each
//! expression here states how one such column is lifted, and every statement
//! that compares a temporal value builds it from these so no two of them
//! disagree.
//!
//! The lift reads text by the same rules every other path does. Its shape check
//! is the lexicon's own pattern of the clause 4.6.3 form
//! ([`TimeValueForm::lexical_pattern`]), so a value the broker classifies as a
//! `DateTime` or a `Time` is exactly one the lift lifts, and text the clause
//! refuses (a numeric offset, a lowercase designator, a seventh fractional
//! digit, a missing component) stays text here as it does everywhere else.
//!
//! Every expression is guarded before it casts. A document member is only
//! *supposed* to hold a temporal literal (nothing in the database enforces it),
//! and an unguarded `::timestamptz` cast over a row that holds `"soon"` aborts
//! the whole statement with a runtime cast error rather than declining to match
//! that one row. The guard is two checks, both of which have to pass:
//!
//! * the shape check above, which also keeps PostgreSQL's far more permissive
//!   timestamp grammar from admitting `now`, `epoch`, a bare date, or a
//!   `2024-01-01 BC` era marker;
//! * `pg_input_is_valid` (PostgreSQL 16+) then asks the `timestamptz` input
//!   function itself whether the shaped text is a real instant, without
//!   raising. A string can satisfy the pattern and still not be one:
//!   `2024-02-30T00:00:00Z` has the right shape but names a day February does
//!   not have.
//!
//! The `timestamptz` input function refuses the clause 4.6.3 compatibility
//! comma, so both checks and the cast read the text with the comma written as
//! the decimal point.
//!
//! Together the two checks turn a non-conforming row into `NULL`, which every
//! comparison then reports as unknown: the row does not match, in either
//! polarity, which is what clause 4.9 requires of a target the operator has no
//! meaning for. Neither check can itself raise an error, so the `CASE`'s `WHEN`
//! is always safe to evaluate, whatever order PostgreSQL chooses to evaluate
//! its two conjuncts in.

use andromeda_ngsi_ld_lexicon::time::value::TimeValueForm;
use andromeda_postgresql_seaquery::{
    cast::cast_compound,
    datetime::{pg_input_is_valid, to_char, with_decimal_point},
    literal::text_literal,
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
};
use sea_query::{CaseStatement, ExprTrait, SimpleExpr};

/// The date a bare time of day is anchored to while it is read as a
/// `timestamptz`, carrying the `T` that joins it to the time of day.
///
/// Discarded immediately afterwards: only the time of day survives `to_char`.
/// The designator is always `Z`, so the date cannot shift the time of day.
const TIME_ANCHOR: &str = "1970-01-01T";

/// The `to_char` template of the stored time of day spelling, `HH:MM:SS.ffffff`,
/// to which the designator `Z` is appended.
///
/// It is the fixed-width spelling `NgsiLdTime` stores, always six fractional
/// digits, so byte-wise order over the lifted text is chronological order.
pub(crate) const STORED_TIME_TEMPLATE: &str = "HH24:MI:SS.US";

/// Wrap a transformation in the two-part guard that makes it safe to apply.
///
/// `shaped` is the stored text the pattern reads, `cast_source` the exact
/// expression `pg_input_is_valid` validates (the same text the cast inside
/// `transformed` sees), so a caller whose cast source is not the stored text
/// itself (the `Time` lift anchors it to a date first) states that explicitly.
fn shape_guarded(shaped: SimpleExpr, form: TimeValueForm, cast_source: SimpleExpr, transformed: SimpleExpr) -> SimpleExpr {
    let guard = shaped
        .posix_matches(text_literal(form.lexical_pattern()))
        .and(pg_input_is_valid(cast_source, PostgresType::Timestamptz));
    CaseStatement::new().case(guard, transformed).into()
}

/// Stored text lifted to `timestamptz`, or `NULL` when it is not a clause 4.6.3
/// `DateTime`.
#[must_use]
pub fn stored_date_time(stored: SimpleExpr) -> SimpleExpr {
    // The stored text is read by the shape check; the comma-free reading of it
    // by `pg_input_is_valid` and by the cast, so each reader takes its own copy.
    let dotted = with_decimal_point(stored.clone());
    let cast_source = dotted.clone();
    let transformed = cast_compound(dotted, PostgresType::Timestamptz);
    shape_guarded(stored, TimeValueForm::DateTime, cast_source, transformed)
}

/// Stored text written in the fixed-width `HH:MM:SS.ffffffZ` spelling, or
/// `NULL` when it is not a clause 4.6.3 `Time`.
///
/// The result is text rather than a `time` value on purpose: the spelling is
/// fixed-width and UTC, so byte-wise text order over it is chronological order,
/// and the comparison is then the same one a stored row makes against its
/// `COLLATE "C"` string column. The anchored text is what is validated and
/// cast, so an out-of-range hour or minute (`25:00:00Z`) is caught by the input
/// function, not only the shape.
#[must_use]
pub fn stored_time(stored: SimpleExpr) -> SimpleExpr {
    // The shape check reads the bare stored text; the validation and the cast
    // read it anchored to a date with its comma written as the decimal point.
    let anchored = text_literal(TIME_ANCHOR).concat_text(with_decimal_point(stored.clone()));
    let cast_source = anchored.clone();
    let normalised = to_char(
        cast_compound(anchored, PostgresType::Timestamptz).at_time_zone(text_literal("UTC")),
        STORED_TIME_TEMPLATE,
    )
    .concat_text(text_literal("Z"));
    shape_guarded(stored, TimeValueForm::Time, cast_source, normalised)
}

#[cfg(test)]
mod tests {
    use crate::temporal_operand::{STORED_TIME_TEMPLATE, stored_date_time, stored_time};
    use andromeda_ngsi_ld_lexicon::time::value::TimeValueForm;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query, SimpleExpr};

    /// The stored operand every test lifts: a plain column stands in for the
    /// `#>> '{}'` extraction the real callers pass.
    fn stored() -> SimpleExpr {
        Expr::col(Alias::new("stored"))
    }

    fn render(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    /// The lift classifies by the lexicon's own pattern, so it lifts exactly
    /// what every other path reads as a `DateTime` or a `Time`.
    #[test]
    fn each_lift_is_guarded_by_the_lexicons_pattern_of_its_form() {
        let date_time = render(stored_date_time(stored()));
        assert!(
            date_time.contains(&format!(r#""stored" ~ ('{}')"#, TimeValueForm::DateTime.lexical_pattern())),
            "{date_time}"
        );
        let time = render(stored_time(stored()));
        assert!(time.contains(&format!(r#""stored" ~ ('{}')"#, TimeValueForm::Time.lexical_pattern())), "{time}");
    }

    #[test]
    fn a_date_time_lift_validates_and_casts_the_comma_free_text() {
        let sql = render(stored_date_time(stored()));
        assert!(sql.contains(r#" AND pg_input_is_valid(replace("stored", ',', '.'), 'timestamptz')"#), "{sql}");
        assert!(sql.contains(r#"THEN (replace("stored", ',', '.'))::timestamptz"#), "{sql}");
    }

    #[test]
    fn a_time_lift_validates_the_anchored_comma_free_text() {
        let sql = render(stored_time(stored()));
        assert!(
            sql.contains(r#"pg_input_is_valid(('1970-01-01T') || replace("stored", ',', '.'), 'timestamptz')"#),
            "{sql}"
        );
    }

    #[test]
    fn a_time_lift_renders_the_fixed_width_stored_spelling() {
        let sql = render(stored_time(stored()));
        let anchored = sql.rfind("'1970-01-01T'").expect("the time of day is anchored");
        let utc = sql.find("AT TIME ZONE ('UTC')").expect("the instant is read in UTC");
        let rendered = sql.find(&format!("'{STORED_TIME_TEMPLATE}'")).expect("only the time of day survives");
        assert!(anchored < utc && utc < rendered, "{sql}");
        assert!(sql.contains("|| ('Z')"), "{sql}");
    }

    #[test]
    fn both_lifts_return_null_rather_than_failing_on_text_of_another_shape() {
        // No ELSE branch: a CASE with no matching arm yields NULL, and every
        // comparison against NULL is unknown, so the row simply does not match.
        for sql in [render(stored_date_time(stored())), render(stored_time(stored()))] {
            assert!(!sql.contains("ELSE"), "{sql}");
            assert!(sql.contains("END"), "{sql}");
        }
    }
}
