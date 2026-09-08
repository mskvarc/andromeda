//! `TimescaleDB` functions sea-query has no form for.
//!
//! `TimescaleDB` extends PostgreSQL with hypertable-management and time-bucketing
//! functions that take keyword arguments (`older_than => …`) and stand in
//! statement positions sea-query models no other way. They are spelled here, in
//! the one crate sanctioned to write a dialect fragment, with every
//! caller-supplied value passed as a bound expression rather than interpolated.

use crate::{
    cast::{cast, cast_compound},
    literal::text_literal,
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
};
use sea_query::{Expr, ExprTrait, Func, Iden, Query, SelectStatement, SimpleExpr};

/// The `time_bucket(width, ts, origin)` bucketing function.
#[derive(Iden, Clone, Copy)]
#[iden = "time_bucket"]
struct TimeBucket;

/// `TimescaleDB`: `time_bucket(width, ts, origin)` - the start of the fixed-width
/// window `ts` falls in, aligned so one bucket boundary lands on `origin`.
///
/// All three arguments are expressions: `width` is an `interval`, `ts` the time
/// axis column being bucketed, and `origin` either a bound reference instant or a
/// correlated per-entity earliest observation. None is interpolated - a
/// caller-supplied period reaches `width` as a bound value already cast to
/// `interval`.
#[must_use]
pub fn time_bucket(width: SimpleExpr, ts: SimpleExpr, origin: SimpleExpr) -> SimpleExpr {
    Func::cust(TimeBucket).args([width, ts, origin]).into()
}

/// `TimescaleDB`: `drop_chunks(hypertable, older_than => cutoff::timestamptz)` -
/// drop every chunk of `hypertable` whose range ends before `cutoff`.
///
/// A set-returning function naming each dropped chunk, so it stands in a
/// projection whose row count is the number of chunks removed. The hypertable is
/// a fixed catalog name rendered inline; the cutoff is a bound value.
#[must_use]
pub fn drop_chunks(hypertable: &str, cutoff: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs(
        "drop_chunks($1, older_than => $2)",
        [text_literal(hypertable), cast(cutoff, PostgresType::Timestamptz)],
    )
}

/// The `timescaledb_information` schema every catalogue view below lives in.
#[derive(Iden, Clone, Copy)]
#[iden = "timescaledb_information"]
struct TimescaleInformation;

/// The `timescaledb_information.dimensions` view.
#[derive(Iden, Clone, Copy)]
enum Dimensions {
    /// View name: `dimensions`
    #[iden = "dimensions"]
    Table,
    /// The hypertable the dimension partitions
    HypertableName,
    /// Ordinal of the dimension, `1` for the time dimension
    DimensionNumber,
    /// The interval one chunk spans on a time dimension
    TimeInterval,
}

/// The `timescaledb_information.jobs` view.
#[derive(Iden, Clone, Copy)]
enum Jobs {
    /// View name: `jobs`
    #[iden = "jobs"]
    Table,
    /// The hypertable the job acts on
    HypertableName,
    /// The procedure the job runs
    ProcName,
    /// The job's JSONB configuration
    Config,
}

/// The procedure name a columnstore policy job runs under.
const COLUMNSTORE_POLICY_PROC: &str = "policy_compression";

/// The `config` key that carries a columnstore policy's lag.
const COMPRESS_AFTER_KEY: &str = "compress_after";

/// The interval one chunk of `hypertable` spans on its time dimension.
///
/// Reads `timescaledb_information.dimensions` for the first dimension, which is
/// the time dimension every hypertable in this schema is partitioned by, and
/// projects its `time_interval`. The hypertable name is a fixed catalogue name
/// bound as a value.
#[must_use]
pub fn chunk_interval_statement(hypertable: &str) -> SelectStatement {
    Query::select()
        .column(Dimensions::TimeInterval)
        .from((TimescaleInformation, Dimensions::Table))
        .and_where(Expr::col(Dimensions::HypertableName).eq(hypertable))
        .and_where(Expr::col(Dimensions::DimensionNumber).eq(1))
        .to_owned()
}

/// The lag of the columnstore policy on `hypertable`, as an `interval`.
///
/// Reads `timescaledb_information.jobs` for the compression-policy job of the
/// hypertable and projects `config ->> 'compress_after'` cast to `interval`;
/// no row means no policy is installed.
#[must_use]
pub fn columnstore_policy_statement(hypertable: &str) -> SelectStatement {
    Query::select()
        .expr(cast_compound(
            SimpleExpr::from(Expr::col(Jobs::Config)).json_get_text(Expr::val(COMPRESS_AFTER_KEY)),
            PostgresType::Interval,
        ))
        .from((TimescaleInformation, Jobs::Table))
        .and_where(Expr::col(Jobs::HypertableName).eq(hypertable))
        .and_where(Expr::col(Jobs::ProcName).eq(COLUMNSTORE_POLICY_PROC))
        .to_owned()
}

/// `TimescaleDB`: `set_chunk_time_interval(hypertable, make_interval(days => n))`
/// - the interval every chunk created from now on spans.
///
/// The hypertable is a fixed catalogue name rendered inline; the day count is a
/// bound value turned into an `interval` server-side.
#[must_use]
pub fn set_chunk_time_interval(hypertable: &str, days: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("set_chunk_time_interval($1, make_interval(days => $2))", [text_literal(hypertable), days])
}

#[cfg(test)]
mod tests {
    use crate::timescale::{chunk_interval_statement, columnstore_policy_statement, drop_chunks, set_chunk_time_interval};
    use sea_query::{Expr, PostgresQueryBuilder, Query};

    #[test]
    fn the_chunk_interval_is_read_from_the_time_dimension_of_the_named_hypertable() {
        let (sql, values) = chunk_interval_statement("attribute_history").build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"SELECT "time_interval" FROM "timescaledb_information"."dimensions" WHERE "hypertable_name" = $1 AND "dimension_number" = $2"#
        );
        assert_eq!(values.0.len(), 2, "{sql}");
    }

    #[test]
    fn the_columnstore_lag_is_read_from_the_compression_policy_job_as_an_interval() {
        let (sql, values) = columnstore_policy_statement("scope_history").build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"SELECT ("config" ->> $1)::interval FROM "timescaledb_information"."jobs" WHERE "hypertable_name" = $2 AND "proc_name" = $3"#
        );
        assert_eq!(values.0.len(), 3, "{sql}");
    }

    #[test]
    fn the_chunk_interval_is_set_from_a_bound_day_count() {
        let (sql, values) = Query::select()
            .expr(set_chunk_time_interval("attribute_history", Expr::val(7_i32)))
            .to_owned()
            .build(PostgresQueryBuilder);
        assert!(sql.contains("set_chunk_time_interval('attribute_history', make_interval(days => $1))"), "{sql}");
        assert_eq!(values.0.len(), 1, "{sql}");
    }

    #[test]
    fn time_bucket_takes_its_width_axis_and_origin_as_expressions() {
        use crate::timescale::time_bucket;
        use sea_query::Alias;
        let sql = Query::select()
            .expr(time_bucket(
                Expr::cust("'PT1H'::interval"),
                Expr::col(Alias::new("observed_at")),
                Expr::col(Alias::new("origin")),
            ))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert!(sql.contains("time_bucket('PT1H'::interval, \"observed_at\", \"origin\")"), "{sql}");
    }

    #[test]
    fn drop_chunks_names_its_hypertable_and_binds_the_cutoff() {
        let (sql, values) = Query::select()
            .expr(drop_chunks("attribute_history", Expr::val("2026-01-01T00:00:00Z")))
            .to_owned()
            .build(PostgresQueryBuilder);
        assert!(sql.contains("drop_chunks('attribute_history', older_than => "), "{sql}");
        assert!(sql.contains("::timestamptz"), "{sql}");
        assert_eq!(values.0.len(), 1, "only the cutoff is bound: {sql}");
    }
}
