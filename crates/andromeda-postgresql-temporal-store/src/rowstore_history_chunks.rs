//! The `attribute_history` chunks the conversion sweep keeps in the rowstore.
//!
//! The sweep counts a chunk's rows and segments once the chunk passes the
//! conversion horizon, and a chunk whose rows per segment fall below the
//! layout's break-even ([`crate::hypertable_policy::SegmentBreakEven`]) is
//! recorded here instead of converted. A chunk past the horizon receives no
//! appends, so the verdict stands and the count is never repeated. Rows of
//! chunks retention has dropped are pruned on the next sweep.

use andromeda_postgresql_schema::temporal::RowstoreHistoryChunks;
use andromeda_postgresql_seaquery::clock::now;
use chrono::{DateTime, Utc};
use sea_query::{DeleteStatement, Expr, ExprTrait, InsertStatement, OnConflict, Query, SelectStatement, error::Error as SeaQueryError};

/// How many rows a chunk holds and over how many segments.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SegmentCensus {
    /// The rows.
    pub(crate) rows: u64,
    /// The distinct segment-by values among them.
    pub(crate) segments: u64,
}

/// `SELECT range_start FROM rowstore_history_chunks`: the chunks kept in the
/// rowstore, by their start on the time dimension.
pub(crate) fn kept_chunks_statement() -> SelectStatement {
    Query::select()
        .column(RowstoreHistoryChunks::RangeStart)
        .from(RowstoreHistoryChunks::Table)
        .to_owned()
}

/// Record the chunk starting at `range_start` as kept in the rowstore, with
/// the census that decided it. A chunk another replica recorded first keeps
/// that record.
pub(crate) fn keep_statement(range_start: DateTime<Utc>, census: SegmentCensus) -> Result<InsertStatement, SeaQueryError> {
    Ok(Query::insert()
        .into_table(RowstoreHistoryChunks::Table)
        .columns([
            RowstoreHistoryChunks::RangeStart,
            RowstoreHistoryChunks::RowCount,
            RowstoreHistoryChunks::StreamCount,
            RowstoreHistoryChunks::KeptAt,
        ])
        .values([
            Expr::val(range_start),
            Expr::val(saturating_bigint(census.rows)),
            Expr::val(saturating_bigint(census.segments)),
            now(),
        ])?
        .on_conflict(OnConflict::column(RowstoreHistoryChunks::RangeStart).do_nothing().to_owned())
        .to_owned())
}

/// Remove the records of chunks that no longer exist: every record whose
/// `range_start` is not among `chunk_starts`, the starts of the hypertable's
/// chunks as the catalogue lists them.
pub(crate) fn prune_statement(chunk_starts: &[DateTime<Utc>]) -> DeleteStatement {
    Query::delete()
        .from_table(RowstoreHistoryChunks::Table)
        .and_where(Expr::col(RowstoreHistoryChunks::RangeStart).is_not_in(chunk_starts.iter().copied()))
        .to_owned()
}

/// A count as the `bigint` column stores it; a chunk never holds more rows.
fn saturating_bigint(count: u64) -> i64 {
    i64::try_from(count).unwrap_or(i64::MAX)
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::rowstore_history_chunks::{SegmentCensus, keep_statement, kept_chunks_statement, prune_statement};
    use andromeda_ngsi_ld_lexicon::time::date_time::NgsiLdDateTime;
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    fn instant(text: &str) -> DateTime<Utc> {
        text.parse::<NgsiLdDateTime>().expect("an RFC 3339 instant").get()
    }

    #[test]
    fn the_kept_chunks_are_read_by_their_start() {
        assert_eq!(
            kept_chunks_statement().to_string(PostgresQueryBuilder),
            r#"SELECT "range_start" FROM "rowstore_history_chunks""#
        );
    }

    #[test]
    fn a_kept_chunk_is_recorded_once_with_its_census() {
        let (sql, values) = keep_statement(
            instant("2026-10-01T00:00:00Z"),
            SegmentCensus {
                rows: 14_932_708,
                segments: 9_883_184,
            },
        )
        .expect("four values for four columns")
        .build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            concat!(
                r#"INSERT INTO "rowstore_history_chunks" ("range_start", "row_count", "stream_count", "kept_at") "#,
                r#"VALUES ($1, $2, $3, NOW()) ON CONFLICT ("range_start") DO NOTHING"#
            )
        );
        assert_eq!(values.0.len(), 3, "{sql}");
    }

    #[test]
    fn records_of_chunks_the_catalogue_no_longer_lists_are_pruned() {
        let (sql, values) = prune_statement(&[instant("2026-10-01T00:00:00Z"), instant("2026-10-08T00:00:00Z")]).build(PostgresQueryBuilder);
        assert_eq!(sql, r#"DELETE FROM "rowstore_history_chunks" WHERE "range_start" NOT IN ($1, $2)"#);
        assert_eq!(values.0.len(), 2, "{sql}");
    }
}
