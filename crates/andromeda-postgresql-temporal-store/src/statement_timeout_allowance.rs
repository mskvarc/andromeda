//! Letting one transaction of work no request waits for run to its end.
//!
//! The broker installs a `statement_timeout` on each database just past the
//! HTTP request deadline. Retention and its write-record release remove as
//! much history as has aged out, however long that takes, and no request
//! waits on them, so each of their transactions lifts the timeout for itself.

use andromeda_postgresql_seaquery::setting::lift_statement_timeout;
use sea_query::{PostgresQueryBuilder, Query};
use sea_query_sqlx::SqlxBinder;
use sqlx::{AssertSqlSafe, PgConnection};

/// Lift the statement timeout until the current transaction ends.
///
/// # Errors
///
/// Returns the driver error when the statement fails.
pub(crate) async fn allow_unbounded_statements(connection: &mut PgConnection) -> Result<(), sqlx::Error> {
    let (sql, values) = Query::select().expr(lift_statement_timeout()).build_sqlx(PostgresQueryBuilder);
    sqlx::query_with(AssertSqlSafe(sql), values).execute(connection).await?;
    Ok(())
}
