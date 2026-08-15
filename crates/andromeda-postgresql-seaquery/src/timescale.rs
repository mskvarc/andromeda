//! TimescaleDB functions sea-query has no form for.
//!
//! TimescaleDB extends PostgreSQL with hypertable-management and time-bucketing
//! functions that take keyword arguments (`older_than => …`) and stand in
//! statement positions sea-query models no other way. They are spelled here, in
//! the one crate sanctioned to write a dialect fragment, with every
//! caller-supplied value passed as a bound expression rather than interpolated.

use crate::{cast::cast, literal::text_literal, pg_type::PostgresType};
use sea_query::{Expr, Func, Iden, SimpleExpr};

/// The `time_bucket(width, ts, origin)` bucketing function.
#[derive(Iden, Clone, Copy)]
#[iden = "time_bucket"]
struct TimeBucket;

/// TimescaleDB: `time_bucket(width, ts, origin)` - the start of the fixed-width
/// window `ts` falls in, aligned so one bucket boundary lands on `origin`.
///
/// All three arguments are expressions: `width` is an `interval`, `ts` the time
/// axis column being bucketed, and `origin` either a bound reference instant or a
/// correlated per-entity earliest observation. None is interpolated - a
/// caller-supplied period reaches `width` as a bound value already cast to
/// `interval`.
pub fn time_bucket(width: SimpleExpr, ts: SimpleExpr, origin: SimpleExpr) -> SimpleExpr {
    Func::cust(TimeBucket).args([width, ts, origin]).into()
}

/// TimescaleDB: `drop_chunks(hypertable, older_than => cutoff::timestamptz)` -
/// drop every chunk of `hypertable` whose range ends before `cutoff`.
///
/// A set-returning function naming each dropped chunk, so it stands in a
/// projection whose row count is the number of chunks removed. The hypertable is
/// a fixed catalog name rendered inline; the cutoff is a bound value.
pub fn drop_chunks(hypertable: &str, cutoff: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs(
        "drop_chunks($1, older_than => $2)",
        [text_literal(hypertable), cast(cutoff, PostgresType::Timestamptz)],
    )
}

#[cfg(test)]
mod tests {
    use crate::timescale::drop_chunks;
    use sea_query::{Expr, PostgresQueryBuilder, Query};

    #[test]
    fn time_bucket_takes_its_width_axis_and_origin_as_expressions() {
        use crate::timescale::time_bucket;
        use sea_query::Alias;
        let sql = Query::select()
            .expr(time_bucket(
                Expr::cust("'PT1H'::interval"),
                Expr::col(Alias::new("observed_at")).into(),
                Expr::col(Alias::new("origin")).into(),
            ))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert!(sql.contains("time_bucket('PT1H'::interval, \"observed_at\", \"origin\")"), "{sql}");
    }

    #[test]
    fn drop_chunks_names_its_hypertable_and_binds_the_cutoff() {
        let (sql, values) = Query::select()
            .expr(drop_chunks("attribute_history", Expr::val("2026-01-01T00:00:00Z").into()))
            .to_owned()
            .build(PostgresQueryBuilder);
        assert!(sql.contains("drop_chunks('attribute_history', older_than => "), "{sql}");
        assert!(sql.contains("::timestamptz"), "{sql}");
        assert_eq!(values.0.len(), 1, "only the cutoff is bound: {sql}");
    }
}
