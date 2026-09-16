//! PostgreSQL date-field functions for calendar and cyclic time bucketing.
//!
//! `date_trunc(field, ts)` truncates a timestamp to the start of a calendar
//! field (a contiguous bucket: one interval of the timeline per group), and
//! `date_part(field, ts)` — the function form of `EXTRACT(field FROM ts)` —
//! projects one repeating field of a timestamp (a cyclic bucket: one group
//! gathers instants from many periods). Neither has a native sea-query
//! constructor, so both are spelled here as [`Func::cust`] in the one crate
//! sanctioned to write a dialect fragment. The field is always a whitelisted
//! literal from a typed bucket token, never caller-supplied text, and the
//! timestamp is a bound expression: nothing is interpolated.

use crate::literal::text_literal;
use sea_query::{Func, Iden, SimpleExpr};

/// The `date_trunc(field, ts)` truncation function.
#[derive(Iden, Clone, Copy)]
#[iden = "date_trunc"]
struct DateTrunc;

/// The `date_part(field, ts)` field-extraction function.
#[derive(Iden, Clone, Copy)]
#[iden = "date_part"]
struct DatePart;

/// `date_trunc('field', ts)` — the start of the calendar `field` interval that
/// `ts` falls in (a contiguous bucket).
///
/// The field names a calendar unit (`year`, `month`, `week`, `day`, `hour`) and
/// is a statement-level constant chosen from a typed token, so it is inlined as
/// a literal exactly as a cast names its type. `ts` is a bound expression.
#[must_use]
pub fn date_trunc(field: &str, ts: SimpleExpr) -> SimpleExpr {
    Func::cust(DateTrunc).args([text_literal(field), ts]).into()
}

/// `date_part('field', ts)` — one repeating `field` of `ts` (a cyclic bucket),
/// the function form of `EXTRACT(field FROM ts)`.
///
/// The field names a repeating unit (`dow`, `hour`, `month`) and is a
/// statement-level constant chosen from a typed token, so it is inlined as a
/// literal. `ts` is a bound expression.
#[must_use]
pub fn date_part(field: &str, ts: SimpleExpr) -> SimpleExpr {
    Func::cust(DatePart).args([text_literal(field), ts]).into()
}

#[cfg(test)]
mod tests {
    use crate::date_field::{date_part, date_trunc};
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query, SimpleExpr};

    fn render(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    #[test]
    fn date_trunc_inlines_its_calendar_field() {
        let sql = render(date_trunc("month", Expr::col(Alias::new("observed_at"))));
        assert!(sql.contains("date_trunc('month', \"observed_at\")"), "{sql}");
    }

    #[test]
    fn date_part_inlines_its_cyclic_field() {
        let sql = render(date_part("hour", Expr::col(Alias::new("observed_at"))));
        assert!(sql.contains("date_part('hour', \"observed_at\")"), "{sql}");
    }
}
