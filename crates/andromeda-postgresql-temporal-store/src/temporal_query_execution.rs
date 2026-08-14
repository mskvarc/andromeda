//! Running one compiled temporal statement against the tenant pool.

use crate::{error::TemporalStoreError, repo::TemporalRepository};
use andromeda_postgresql_seaquery::bind::BindValue;

impl TemporalRepository {
    /// Execute a compiled temporal statement with its typed bind values.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when the statement fails.
    pub(crate) async fn execute_temporal_query(&self, sql: &str, binds: &[BindValue<'_>]) -> Result<Vec<sqlx::postgres::PgRow>, TemporalStoreError> {
        let query = andromeda_postgresql_seaquery::bind_values!(sqlx::query(sql), binds);
        Ok(query.fetch_all(&self.pool).await?)
    }
}
