//! Whether text a document keeps as its writer wrote it spells a clause 4.6.3
//! time value.
//!
//! A registration's Context Source Properties are stored as written, so no
//! lexical reader has read the text a declared value or an instant member
//! holds, and the in-memory evaluator reads it as a time value only in the
//! clause 4.6.3 form of its datatype (pp. 80 and 81): every component present,
//! the literal `Z`, at most six fractional digits after a point or the comma
//! clause 4.6.3 admits in requests, a real calendar day. PostgreSQL's input
//! functions read more (an offset, a bare date, an unpadded month, `now`) and
//! no decimal comma, so a text is matched against the form's pattern before an
//! input function checks its day, and its comma is read as the point
//! ([`with_decimal_point`]) wherever an input function reads it.

use andromeda_ngsi_ld_lexicon::time::value::TimeValueForm;
use andromeda_postgresql_seaquery::{
    datetime::{pg_input_is_valid, with_decimal_point},
    literal::text_literal,
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
};
use sea_query::{ExprTrait, SimpleExpr};

/// Whether `text` spells a time value of `form` in its clause 4.6.3 form,
/// `text` naming the written text as often as the test reads it.
///
/// A `DateTime` and a `Date` match their pattern and name a real day, which
/// the `timestamptz` and `date` input functions check; a `Time` matches its
/// pattern, which bounds the clock.
#[must_use]
pub fn spells(form: TimeValueForm, text: impl Fn() -> SimpleExpr) -> SimpleExpr {
    let written_in_form = text().posix_matches(text_literal(form.lexical_pattern()));
    match form {
        TimeValueForm::DateTime => written_in_form.and(pg_input_is_valid(with_decimal_point(text()), PostgresType::Timestamptz)),
        TimeValueForm::Date => written_in_form.and(pg_input_is_valid(text(), PostgresType::Date)),
        TimeValueForm::Time => written_in_form,
    }
}

#[cfg(test)]
mod tests {
    use crate::q_predicate::document::written_time_value::spells;
    use andromeda_ngsi_ld_lexicon::time::value::TimeValueForm;
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use sea_query::{Alias, Expr, Query, SimpleExpr};

    fn written() -> SimpleExpr {
        Expr::col(Alias::new("written"))
    }

    /// Clause 4.6.3 pins each form: a text matches the form's pattern, and a
    /// `DateTime` (its comma read as the point) or a `Date` names a day the
    /// calendar has.
    #[test]
    fn each_form_matches_its_pattern_and_a_dated_form_its_calendar() {
        for (index, form) in [TimeValueForm::DateTime, TimeValueForm::Date, TimeValueForm::Time].into_iter().enumerate() {
            assert_statement_snapshot!(
                format!("written-time-value-each-form-matches-its-pattern-and-a-dated-form-its-calendar-{}", index + 1),
                Query::select().expr(spells(form, written)).to_owned()
            );
        }
    }
}
