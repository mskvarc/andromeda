//! Letting one transaction decompress what its own statements must.
//!
//! An `UPDATE` or `DELETE` that reaches a converted chunk decompresses every
//! compressed batch its bound values cannot rule out, and TimescaleDB refuses
//! the statement once a transaction has decompressed
//! `timescaledb.max_tuples_decompressed_per_dml_transaction` rows (100,000 by
//! default). The limit is a guard against an unbounded statement; a removal
//! whose bound values already confine it to the addressed streams and window
//! decompresses only the batches that hold rows it must remove or keep, so the
//! guard could only turn requested work into a failure. Such a transaction
//! lifts it for its own duration, and nothing else.

use crate::error::TemporalStoreError;
use andromeda_postgresql_seaquery::setting::{SettingScope, set_config};
use sea_query::{PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::{AssertSqlSafe, PgConnection};

/// The `TimescaleDB` setting bounding the rows one transaction's DML may
/// decompress; zero lifts the bound.
const DECOMPRESSION_LIMIT_SETTING: &str = "timescaledb.max_tuples_decompressed_per_dml_transaction";

/// Lift the decompression limit until the current transaction ends.
///
/// On a server without the columnstore the setting is an inert placeholder.
///
/// # Errors
///
/// Returns [`TemporalStoreError::Database`] when the statement fails.
pub(crate) async fn allow_confined_decompression(connection: &mut PgConnection) -> Result<(), TemporalStoreError> {
    let (sql, values) = allowance_statement().build_sqlx(PostgresQueryBuilder);
    sqlx::query_with(AssertSqlSafe(sql), values).execute(connection).await?;
    Ok(())
}

/// `SELECT set_config(limit, '0', true)`.
fn allowance_statement() -> SelectStatement {
    Query::select()
        .expr(set_config(DECOMPRESSION_LIMIT_SETTING, "0", SettingScope::Transaction))
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::decompression_allowance::allowance_statement;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn the_allowance_lifts_the_decompression_limit_for_the_transaction_alone() {
        assert_eq!(
            allowance_statement().to_string(PostgresQueryBuilder),
            "SELECT set_config('timescaledb.max_tuples_decompressed_per_dml_transaction', '0', TRUE)"
        );
    }
}
