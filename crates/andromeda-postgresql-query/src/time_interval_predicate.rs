//! A clause 4.11 time interval as a predicate on one time column.
//!
//! `before` is an exclusive bound, `after` an inclusive one, and `between`
//! includes its lower bound and excludes its upper one. Every statement that
//! reads a Temporal Evolution applies the same reading to whichever column holds
//! the requested Temporal Property, so it is stated once.

use andromeda_ngsi_ld_query::temporal::window::TimeInterval;
use sea_query::{Condition, Expr as SeaExpr, ExprTrait, SimpleExpr};

/// Whether `column` lies inside `interval` (clause 4.11).
///
/// An unbounded interval admits every row, including one whose column is NULL;
/// every bounded one leaves out a row that carries no instant on the column,
/// because clause 4.11 makes an Entity "which do[es] not convey the target
/// Temporal Property of the query" non-matching.
#[must_use]
pub fn within_interval(column: &SimpleExpr, interval: &TimeInterval) -> Condition {
    // clone: sea-query owns every operand, and one column is compared against each bound
    let bound = || SeaExpr::expr(column.clone());
    match interval {
        TimeInterval::Unbounded => Condition::all(),
        TimeInterval::Before(instant) => Condition::all().add(bound().lt(*instant)),
        TimeInterval::After(instant) => Condition::all().add(bound().gte(*instant)),
        TimeInterval::Between { from, to } => Condition::all().add(bound().gte(*from)).add(bound().lt(*to)),
    }
}

#[cfg(test)]
mod tests {
    use crate::time_interval_predicate::within_interval;
    use andromeda_ngsi_ld_lexicon::time::date_time::NgsiLdDateTime;
    use andromeda_ngsi_ld_query::temporal::window::TimeInterval;
    use chrono::{DateTime, Utc};
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query, SimpleExpr};

    fn instant(raw: &str) -> DateTime<Utc> {
        raw.parse::<NgsiLdDateTime>().expect("a valid instant").get()
    }

    fn render(interval: &TimeInterval) -> String {
        let column: SimpleExpr = Expr::col((Alias::new("h"), Alias::new("observed_at")));
        Query::select()
            .expr(Expr::val(1))
            .cond_where(within_interval(&column, interval))
            .to_owned()
            .to_string(PostgresQueryBuilder)
    }

    /// Clause 4.11: `before` excludes its bound, `after` includes it, and
    /// `between` includes its lower bound and excludes its upper one.
    #[test]
    fn each_relation_renders_the_bounds_clause_4_11_gives_it() {
        let at = instant("2024-01-01T00:00:00Z");
        let end = instant("2024-02-01T00:00:00Z");
        assert!(render(&TimeInterval::Before(at)).contains(r#""h"."observed_at" < '2024-01-01"#));
        assert!(render(&TimeInterval::After(at)).contains(r#""h"."observed_at" >= '2024-01-01"#));
        let between = render(&TimeInterval::Between { from: at, to: end });
        assert!(between.contains(r#""h"."observed_at" >= '2024-01-01"#), "{between}");
        assert!(between.contains(r#""h"."observed_at" < '2024-02-01"#), "{between}");
    }

    #[test]
    fn an_unbounded_interval_constrains_no_column() {
        let sql = render(&TimeInterval::Unbounded);
        assert!(!sql.contains("observed_at"), "{sql}");
    }
}
