//! Lifting stored temporal text out of JSONB into the domain clause 4.9 orders
//! it in.
//!
//! A `DateTime`, `Date` or `Time` read out of a JSONB document with `#>>`
//! arrives as whatever text the client wrote. Clause 4.6.3 admits many
//! spellings of one instant (any offset, any fractional width), so text
//! comparison over that is not the temporal comparison clause 4.9 asks for.
//! Each expression here states how one such column is lifted, and every
//! statement that compares a temporal value builds it from these so no two of
//! them disagree.
//!
//! Every expression is guarded before it casts. A document member is only
//! *supposed* to hold a temporal literal (nothing in the database enforces it),
//! and an unguarded `::timestamptz` cast over a row that holds `"soon"` aborts
//! the whole statement with a runtime cast error rather than declining to match
//! that one row. The guard is two checks, both of which have to pass:
//!
//! * a regular expression restricts the *shape* to clause 4.6.3's RFC 3339
//!   productions, so PostgreSQL's far more permissive timestamp grammar cannot
//!   admit `now`, `epoch`, a bare date, or a `2024-01-01 BC` era marker, none of
//!   which the client wrote as a `DateTime` or `Time` literal;
//! * `pg_input_is_valid`
//!   (PostgreSQL 16+) then asks the `timestamptz` input function itself whether
//!   the shaped text is a real instant, without raising. A string can satisfy
//!   the regex and still not be one: `2024-02-30T00:00:00Z` has the right
//!   shape but names a day February does not have, and `25:00:00Z` has the right
//!   shape but names an hour a day does not have. Only the input function
//!   knows every calendar and clock rule the regex does not encode.
//!
//! Together the two checks turn a non-conforming row into `NULL`, which every
//! comparison then reports as unknown: the row does not match, in either
//! polarity, which is what clause 4.9 requires of a target the operator has no
//! meaning for. Neither check can itself raise an error (the regex either
//! matches or does not, and `pg_input_is_valid` catches the input function's
//! error internally), so the `CASE`'s `WHEN` is always safe to evaluate,
//! whatever order PostgreSQL chooses to evaluate its two conjuncts in.

use andromeda_postgresql_seaquery::{
    cast::cast_compound,
    datetime::{pg_input_is_valid, to_char},
    literal::text_literal,
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
};
use sea_query::{CaseStatement, ExprTrait, SimpleExpr};

/// The RFC 3339 `date-time` shape a `::timestamptz` cast is allowed to see.
///
/// Anchored at both ends and offset-bearing: PostgreSQL's own timestamp parser is
/// far more permissive than clause 4.6.3 (`now`, `epoch`, `2024-01-01 BC` and
/// bare dates all cast), and admitting those would compare a stored value against
/// an instant the client never wrote.
const DATE_TIME_SHAPE: &str = "^[0-9]{4}-[0-9]{2}-[0-9]{2}[Tt][0-9]{2}:[0-9]{2}:[0-9]{2}([.][0-9]+)?([Zz]|[+-][0-9]{2}:[0-9]{2})$";

/// The RFC 3339 `full-time` shape the time normalisation is allowed to see.
///
/// The offset is mandatory, exactly as clause 4.6.3's `full-time` production
/// makes it: without one there is no time of day to normalise to UTC.
const TIME_SHAPE: &str = "^[0-9]{2}:[0-9]{2}:[0-9]{2}([.][0-9]+)?([Zz]|[+-][0-9]{2}:[0-9]{2})$";

/// The date a bare `full-time` is anchored to while its offset is resolved,
/// carrying the `T` that joins it to the time of day.
///
/// Discarded immediately afterwards: only the time of day survives `to_char`. An
/// offset shifts every date by the same amount, so which date it is cannot change
/// the result.
const TIME_ANCHOR: &str = "1970-01-01T";

/// Wrap a transformation in the two-part guard that makes it safe to apply.
///
/// `regex_operand` is evaluated by the regex, `cast_source` is the exact
/// expression `pg_input_is_valid` validates (the same text the real cast inside
/// `transformed` sees), so a caller whose cast source is not the bare stored
/// expression (the `Time` lift anchors it to a date first) states that
/// explicitly rather than the guard assuming they are the same text.
fn shape_guarded(regex_operand: SimpleExpr, shape: &str, cast_source: SimpleExpr, transformed: SimpleExpr) -> SimpleExpr {
    let guard = regex_operand
        .posix_matches(text_literal(shape))
        .and(pg_input_is_valid(cast_source, PostgresType::Timestamptz));
    CaseStatement::new().case(guard, transformed).into()
}

/// Stored text lifted to `timestamptz`, or `NULL` when it is not a real RFC 3339
/// `date-time`.
///
/// PostgreSQL resolves the offset itself, so two spellings of one instant compare
/// equal without the statement having to normalise either.
#[must_use]
pub fn stored_date_time(stored: SimpleExpr) -> SimpleExpr {
    // The one stored expression is read three times (by the shape check, by
    // `pg_input_is_valid`, and by the cast), so each reader takes its own copy.
    let regex_operand = stored.clone();
    let cast_source = stored.clone();
    let transformed = cast_compound(stored, PostgresType::Timestamptz);
    shape_guarded(regex_operand, DATE_TIME_SHAPE, cast_source, transformed)
}

/// Stored text normalised to the canonical `HH:MM:SS.ffffffZ` form, or `NULL`
/// when it is not a real RFC 3339 `full-time`.
///
/// The result is text rather than a `time` value on purpose: the canonical form
/// is fixed-width and UTC, so byte-wise text order over it is chronological
/// order, and the comparison is then the same one a stored row makes against
/// its `COLLATE "C"` string column.
///
/// The offset is resolved by anchoring the time of day to a date and letting the
/// `timestamptz` parser do it, then converting to UTC before the date is dropped.
/// The literal carries its own offset, so neither step consults the session
/// `TimeZone` and the result does not depend on the server's configuration. The
/// anchored, concatenated text is what is validated and cast, not the bare
/// stored expression: an out-of-range hour or minute (`25:00:00Z`, `12:60:00Z`)
/// has the `TIME_SHAPE` regex's shape but is not a real time of day, and only
/// validating the text that is actually cast catches that.
#[must_use]
pub fn stored_time(stored: SimpleExpr) -> SimpleExpr {
    // The shape check reads the bare stored text; the offset resolution reads it
    // anchored to a date.
    let regex_operand = stored.clone();
    let anchored = text_literal(TIME_ANCHOR).concat_text(stored);
    // The anchored text is validated by the guard and cast by the transform, so
    // both readers take their own copy.
    let cast_source = anchored.clone();
    let normalised = to_char(
        cast_compound(anchored, PostgresType::Timestamptz).at_time_zone(text_literal("UTC")),
        "HH24:MI:SS.US",
    )
    .concat_text(text_literal("Z"));
    shape_guarded(regex_operand, TIME_SHAPE, cast_source, normalised)
}

#[cfg(test)]
mod tests {
    use crate::temporal_operand::{stored_date_time, stored_time};
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query, SimpleExpr};

    /// The stored operand every test lifts: a plain column stands in for the
    /// `#>> '{}'` extraction the real callers pass.
    fn stored() -> SimpleExpr {
        Expr::col(Alias::new("stored"))
    }

    fn render(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    #[test]
    fn a_date_time_lift_is_guarded_before_it_casts() {
        let sql = render(stored_date_time(stored()));
        assert!(sql.contains("CASE WHEN "), "{sql}");
        assert!(sql.contains("::timestamptz"), "{sql}");
        assert!(sql.contains("~ ('^[0-9]{4}-"), "the cast must not see arbitrary text: {sql}");
        assert!(sql.contains("pg_input_is_valid("), "{sql}");
    }

    #[test]
    fn a_time_lift_resolves_the_offset_before_the_date_is_dropped() {
        // The time of day is anchored to a date so the `timestamptz` parser
        // resolves the offset; only then is the date discarded by `to_char`.
        let sql = render(stored_time(stored()));
        let anchored = sql.find("'1970-01-01T'").expect("the time of day is anchored");
        let utc = sql.find("AT TIME ZONE").expect("the offset is resolved");
        let rendered = sql.find("'HH24:MI:SS.US'").expect("only the time of day survives");
        assert!(anchored < utc && utc < rendered, "{sql}");
    }

    #[test]
    fn a_time_lift_renders_the_fixed_width_canonical_form() {
        let sql = render(stored_time(stored()));
        assert!(sql.contains("'HH24:MI:SS.US'"), "six fractional digits, always: {sql}");
        assert!(sql.contains("|| ('Z')"), "an explicit Zulu offset, always: {sql}");
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

    #[test]
    fn the_time_shape_requires_an_offset_as_clause_four_six_three_does() {
        let sql = render(stored_time(stored()));
        assert!(sql.contains("([Zz]|[+-][0-9]{2}:[0-9]{2})$"), "{sql}");
    }

    #[test]
    fn a_date_time_lift_validates_and_the_guard_conjoins_both_checks() {
        // The regex admits a shaped-but-uncalendarable literal such as
        // `2024-02-30T00:00:00Z`; `pg_input_is_valid` is what actually catches
        // it, and both checks gate the same `WHEN`.
        let sql = render(stored_date_time(stored()));
        assert!(sql.contains("pg_input_is_valid("), "{sql}");
        assert!(sql.contains(" AND pg_input_is_valid"), "both checks gate the same WHEN: {sql}");
    }

    #[test]
    fn a_time_lift_validates_the_anchored_text_not_the_bare_stored_text() {
        // `25:00:00Z` and `12:60:00Z` satisfy `TIME_SHAPE` but are not real
        // times of day; only the anchored, concatenated text that is actually
        // cast is validated, so the anchor date appears inside the validation
        // call.
        let sql = render(stored_time(stored()));
        let validation = sql.find("pg_input_is_valid(").expect("the anchored text is validated");
        let anchor_in_validation = sql[validation..].find("'1970-01-01T'").expect("the validated text is the anchored text");
        let close = sql[validation..].find(", 'timestamptz')").expect("the validation names its type");
        assert!(anchor_in_validation < close, "the anchor is inside the validation call: {sql}");
    }
}
