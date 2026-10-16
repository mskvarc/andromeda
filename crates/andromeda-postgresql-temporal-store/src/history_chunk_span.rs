//! Where the history hypertables' chunks lie, as the write-record release
//! needs to know it.
//!
//! A write record's history rows all carry the record's own `recorded_at`, so
//! they sit in the one chunk of each hypertable that covers that instant.
//! Proving that a record protects nothing is an index probe in a row-store
//! chunk, but a converted chunk keeps no index on the write identity, so the
//! same proof there reads every compressed batch of the chunk. The span read
//! here tells the release which instants it can prove cheaply: those after the
//! latest converted chunk, and those before the earliest chunk, where no
//! history row can exist at all.

use crate::{error::TemporalStoreError, schema_ddl::timescale_policy::HypertableName};
use andromeda_postgresql_seaquery::timescale::chunk_span_statement;
use chrono::{DateTime, Utc};
use sea_query::PostgresQueryBuilder;
use sea_query_sqlx::SqlxBinder;
use sqlx::{AssertSqlSafe, PgConnection, Row};
use strum::IntoEnumIterator;

/// Where the chunks of both history hypertables lie on the write clock.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct HistoryChunkSpan {
    /// The first instant past the latest converted chunk; `None` when no chunk
    /// is converted.
    pub(crate) compressed_until: Option<DateTime<Utc>>,
    /// The first instant any chunk covers; `None` when there is no chunk, and
    /// so no history row, at all.
    pub(crate) history_from: Option<DateTime<Utc>>,
}

impl HistoryChunkSpan {
    /// Read the span of the hypertables named `attribute_history` and
    /// `scope_history`, as every catalogue read of the storage policy names
    /// them.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when the catalogue cannot be
    /// read.
    pub(crate) async fn read(connection: &mut PgConnection) -> Result<HistoryChunkSpan, TemporalStoreError> {
        let (sql, values) = chunk_span_statement(&history_hypertables()).build_sqlx(PostgresQueryBuilder);
        let row = sqlx::query_with(AssertSqlSafe(sql), values).fetch_one(connection).await?;
        Ok(HistoryChunkSpan {
            compressed_until: row.try_get("compressed_until")?,
            history_from: row.try_get("history_from")?,
        })
    }
}

/// The catalogue names of both history hypertables.
fn history_hypertables() -> Vec<&'static str> {
    HypertableName::iter().map(<&'static str>::from).collect()
}

#[cfg(test)]
mod tests {
    use crate::history_chunk_span::history_hypertables;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_span_covers_both_history_hypertables() {
        assert_eq!(history_hypertables(), vec!["attribute_history", "scope_history"]);
    }
}
