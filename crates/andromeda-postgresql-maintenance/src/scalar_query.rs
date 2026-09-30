//! Executing a SeaQuery-built statement that selects one scalar.

use crate::error::MaintenanceError;
use chrono::{DateTime, Utc};
use sea_query::{PostgresQueryBuilder, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::PgConnection;

/// Execute a SeaQuery-built boolean scalar query.
pub(crate) async fn select_bool(connection: &mut PgConnection, statement: SelectStatement) -> Result<bool, MaintenanceError> {
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    Ok(sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_one(connection).await?)
}

/// Execute a SeaQuery-built signed integer scalar query.
pub(crate) async fn select_i64(connection: &mut PgConnection, statement: SelectStatement) -> Result<i64, MaintenanceError> {
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    Ok(sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_one(connection).await?)
}

/// Execute a SeaQuery-built optional instant scalar query.
pub(crate) async fn select_optional_instant(connection: &mut PgConnection, statement: SelectStatement) -> Result<Option<DateTime<Utc>>, MaintenanceError> {
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    Ok(sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_one(connection).await?)
}
