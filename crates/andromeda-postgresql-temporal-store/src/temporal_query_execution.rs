//! Running one compiled temporal statement against the tenant pool.

use crate::{error::TemporalStoreError, repo::TemporalRepository};
use sea_query::{PostgresQueryBuilder, SelectStatement};
use sea_query_binder::SqlxBinder;
use sqlx::postgres::PgRow;

impl TemporalRepository {
    /// Execute a compiled temporal statement tree, binding its values through
    /// `sea-query-binder`.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when the statement fails.
    pub(crate) async fn execute_temporal_query(&self, statement: SelectStatement) -> Result<Vec<PgRow>, TemporalStoreError> {
        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        Ok(sqlx::query_with(&sql, values).fetch_all(&self.pool).await?)
    }
}
