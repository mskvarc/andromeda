//! Lifting a declared time value read out of JSONB into the domain clause 4.9 orders it in.
//!
//! A value is a time value only when it is declared as one (annex C.6), so the
//! lift is guarded by the declaration, not by the shape of the text: `datatype`
//! is the expanded datatype IRI the value carries (a value node's `@type`, or
//! the `valueType` beside it), and the lift yields `NULL` unless it names the
//! lift's form. A declared value is committed in its served spelling, so a
//! `DateTime` reads as an instant and a `Time` is brought to the fixed-width
//! encoding whose byte order is chronological.
//!
//! The cast is also guarded by `pg_input_is_valid` (PostgreSQL 16+), which asks
//! the `timestamptz` input function whether the text is a real instant without
//! raising: a document member is only supposed to hold what admission let in,
//! and an unguarded cast over one that does not aborts the whole statement
//! rather than declining to match that one row. Neither check can raise, so the
//! `CASE`'s `WHEN` is always safe to evaluate, whatever order PostgreSQL
//! evaluates its conjuncts in, and a row that fails either does not match, in
//! either polarity.

use andromeda_ngsi_ld_lexicon::time::value::TimeValueForm;
use andromeda_postgresql_seaquery::{
    cast::cast_compound,
    datetime::{pg_input_is_valid, to_char},
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

/// The `to_char` template of the fixed-width storage encoding of a time of
/// day, `HH:MM:SS.ffffff`, to which the designator `Z` is appended.
pub(crate) const STORED_TIME_TEMPLATE: &str = "HH24:MI:SS.US";

/// Whether `datatype` names `form`.
fn declares(datatype: SimpleExpr, form: TimeValueForm) -> SimpleExpr {
    datatype.eq(text_literal(form.iri().as_str()))
}

/// A text declared as a `DateTime`, lifted to `timestamptz`; `NULL` otherwise.
#[must_use]
pub fn declared_date_time(text: SimpleExpr, datatype: SimpleExpr) -> SimpleExpr {
    let guard = declares(datatype, TimeValueForm::DateTime).and(pg_input_is_valid(text.clone(), PostgresType::Timestamptz)); // clone: the guard and the cast read one text
    CaseStatement::new().case(guard, cast_compound(text, PostgresType::Timestamptz)).into()
}

/// A text declared as a `Date`, as itself; `NULL` otherwise.
///
/// A date has one spelling, fixed-width and zero-padded, so text order over it
/// is chronological order.
#[must_use]
pub fn declared_date(text: SimpleExpr, datatype: SimpleExpr) -> SimpleExpr {
    CaseStatement::new().case(declares(datatype, TimeValueForm::Date), text).into()
}

/// A text declared as a `Time`, in its fixed-width storage encoding
/// `HH:MM:SS.ffffffZ`; `NULL` otherwise.
///
/// The result is text rather than a `time` value: the encoding is fixed-width
/// and UTC, so byte-wise order over it is chronological order, the comparison a
/// stored row makes against its `COLLATE "C"` string column.
#[must_use]
pub fn declared_time(text: SimpleExpr, datatype: SimpleExpr) -> SimpleExpr {
    let anchored = text_literal(TIME_ANCHOR).concat_text(text);
    let guard = declares(datatype, TimeValueForm::Time).and(pg_input_is_valid(anchored.clone(), PostgresType::Timestamptz)); // clone: the guard and the cast read one text
    let fixed_width = to_char(
        cast_compound(anchored, PostgresType::Timestamptz).at_time_zone(text_literal("UTC")),
        STORED_TIME_TEMPLATE,
    )
    .concat_text(text_literal("Z"));
    CaseStatement::new().case(guard, fixed_width).into()
}

#[cfg(test)]
mod tests {
    use crate::temporal_operand::{STORED_TIME_TEMPLATE, declared_date, declared_date_time, declared_time};
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query, SimpleExpr};

    fn text() -> SimpleExpr {
        Expr::col(Alias::new("text"))
    }

    fn datatype() -> SimpleExpr {
        Expr::col(Alias::new("datatype"))
    }

    fn render(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    /// Annex C.6: a value is a time value only when it is declared as one, so
    /// each lift reads the declaration and no regular expression.
    #[test]
    fn each_lift_is_guarded_by_the_declared_datatype_of_its_form() {
        for (sql, iri) in [
            (render(declared_date_time(text(), datatype())), "https://uri.etsi.org/ngsi-ld/DateTime"),
            (render(declared_date(text(), datatype())), "https://uri.etsi.org/ngsi-ld/Date"),
            (render(declared_time(text(), datatype())), "https://uri.etsi.org/ngsi-ld/Time"),
        ] {
            assert!(sql.contains(&format!(r#""datatype" = ('{iri}')"#)), "{sql}");
            assert!(!sql.contains('~'), "no regular expression: {sql}");
        }
    }

    #[test]
    fn a_date_time_lift_validates_before_it_casts() {
        let sql = render(declared_date_time(text(), datatype()));
        assert!(sql.contains(r#"pg_input_is_valid("text", 'timestamptz')"#), "{sql}");
        assert!(sql.contains(r#"THEN ("text")::timestamptz"#), "{sql}");
    }

    #[test]
    fn a_time_lift_renders_the_fixed_width_storage_encoding() {
        let sql = render(declared_time(text(), datatype()));
        let anchored = sql.rfind("'1970-01-01T'").expect("the time of day is anchored");
        let utc = sql.find("AT TIME ZONE ('UTC')").expect("the instant is read in UTC");
        let rendered = sql.find(&format!("'{STORED_TIME_TEMPLATE}'")).expect("only the time of day survives");
        assert!(anchored < utc && utc < rendered, "{sql}");
        assert!(sql.contains("|| ('Z')"), "{sql}");
    }

    #[test]
    fn every_lift_returns_null_rather_than_failing_on_an_undeclared_value() {
        for sql in [
            render(declared_date_time(text(), datatype())),
            render(declared_date(text(), datatype())),
            render(declared_time(text(), datatype())),
        ] {
            assert!(!sql.contains("ELSE"), "{sql}");
            assert!(sql.contains("END"), "{sql}");
        }
    }
}
