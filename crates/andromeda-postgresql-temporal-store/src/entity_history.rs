//! Entity-wide temporal-history persistence operations.

use crate::{error::TemporalStoreError, repo::TemporalRepository};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_postgresql_query::bind::BindValue;
use sqlx::Row;

impl TemporalRepository {
    /// Distinct attribute names recorded for the given temporal entity.
    pub(crate) async fn get_attribute_names_for_entity(&self, entity_id: &Uri) -> Result<Vec<AttributeName>, TemporalStoreError> {
        let rows = sqlx::query(
            r#"
            SELECT DISTINCT an.name AS attribute_name
            FROM temporal_entity_attributes tea
            JOIN attribute_names an ON an.id = tea.attribute_name_id
            WHERE tea.entity_id = $1
            "#,
        )
        .bind(entity_id.as_str())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                let raw: String = row.try_get("attribute_name")?;
                raw.parse::<AttributeName>()
                    .map_err(|error| TemporalStoreError::Decode(format!("Invalid attribute_names.name '{raw}': {error}")))
            })
            .collect()
    }

    /// Check whether any temporal data exists for an entity.
    pub(crate) async fn tea_exists_for_entity(&self, entity_id: &Uri) -> Result<bool, TemporalStoreError> {
        let exists = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM temporal_entity_attributes WHERE entity_id = $1)")
            .bind(entity_id.as_str())
            .fetch_one(&self.pool)
            .await?;
        Ok(exists)
    }

    /// Delete all temporal entity attributes for an entity.
    pub(crate) async fn delete_teas_for_entity(&self, entity_id: &Uri) -> Result<u64, TemporalStoreError> {
        let result = sqlx::query("DELETE FROM temporal_entity_attributes WHERE entity_id = $1")
            .bind(entity_id.as_str())
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    /// Execute a compiled temporal history statement.
    pub(crate) async fn execute_temporal_query(&self, sql: &str, binds: &[BindValue<'_>]) -> Result<Vec<sqlx::postgres::PgRow>, TemporalStoreError> {
        let query = andromeda_postgresql_query::bind_values!(sqlx::query(sql), binds);
        Ok(query.fetch_all(&self.pool).await?)
    }
}
