//! `SnapshotStore` trait implementation for `SnapshotRepository`.

use crate::{
    core::error::AndromedaError,
    service::snapshot::store::{InsertSnapshotParams, SnapshotEntityInput, SnapshotStore},
};
use andromeda_ngsi_ld_lexicon::{ngsi_ld_urn::SnapshotId, uri::Uri};
use andromeda_ngsi_ld_model::{
    snapshot::{ExecutionResultDetails, Snapshot, SnapshotPriority, SnapshotStatus},
    snapshot_filter::SnapshotPurgeFilter,
};
use andromeda_postgresql_context_query::{
    snapshot_listing::{SnapshotListingParams, build_get_row_query, build_listing_query},
    snapshot_purge::build_match_query,
};
use andromeda_postgresql_context_store::snapshot::{mapper::row_to_snapshot, params::SnapshotQueryParams, repo::SnapshotRepository};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use iso8601_duration::Duration as Iso8601Duration;
use sea_query::Alias;
use serde_json::Value;
use sqlx::{Row, postgres::PgPool};
use url::Url;

#[async_trait]
impl SnapshotStore for SnapshotRepository {
    async fn create(&self, params: InsertSnapshotParams<'_>) -> Result<(), AndromedaError> {
        sqlx::query(
            "INSERT INTO snapshots (id, snapshot_queries, snapshot_temporal_queries, snapshot_lifetime, snapshot_priority, endpoint, receiver_info, status, queries_details, temporal_queries_details, created_at, modified_at, expires_at, last_used_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8::snapshot_status, $9, $10, $11, $12, $13, $14)",
        )
        .bind(params.id.as_str())
        .bind(params.snapshot_queries_json)
        .bind(params.snapshot_temporal_queries_json)
        .bind(params.snapshot_lifetime)
        .bind(params.snapshot_priority)
        .bind(params.endpoint.map(Url::as_str))
        .bind(params.receiver_info_json)
        .bind(<&str>::from(params.status))
        .bind(params.queries_details_json)
        .bind(params.temporal_queries_details_json)
        .bind(params.created_at)
        .bind(params.modified_at)
        .bind(params.expires_at)
        .bind(params.last_used_at)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    async fn get(&self, id: &SnapshotId) -> Result<Option<Snapshot>, AndromedaError> {
        let row = sqlx::query(
            "SELECT id, snapshot_queries, snapshot_temporal_queries, snapshot_lifetime, snapshot_priority, endpoint, receiver_info, status::text AS status, queries_details, temporal_queries_details, created_at, modified_at, expires_at, last_used_at FROM snapshots WHERE id = $1",
        )
        .bind(id.as_str())
        .fetch_optional(self.pool())
        .await?;

        match row {
            Some(row) => Ok(Some(row_to_snapshot(&row)?)),
            None => Ok(None),
        }
    }

    async fn exists(&self, id: &SnapshotId) -> Result<bool, AndromedaError> {
        let row = sqlx::query("SELECT 1 FROM snapshots WHERE id = $1")
            .bind(id.as_str())
            .fetch_optional(self.pool())
            .await?;
        Ok(row.is_some())
    }

    async fn update_lifetime_priority(
        &self,
        id: &SnapshotId,
        lifetime: Option<&Iso8601Duration>,
        priority: Option<SnapshotPriority>,
        new_expires_at: Option<DateTime<Utc>>,
    ) -> Result<u64, AndromedaError> {
        let result = sqlx::query(
            "UPDATE snapshots SET snapshot_lifetime = COALESCE($2, snapshot_lifetime), snapshot_priority = COALESCE($3, snapshot_priority), expires_at = COALESCE($4, expires_at), modified_at = NOW() WHERE id = $1",
        )
        .bind(id.as_str())
        .bind(lifetime.map(ToString::to_string))
        .bind(priority.map(|p| i16::from(p.as_u8())))
        .bind(new_expires_at)
        .execute(self.pool())
        .await?;
        Ok(result.rows_affected())
    }

    async fn update_status(
        &self,
        id: &SnapshotId,
        status: SnapshotStatus,
        queries_details: Option<&[ExecutionResultDetails]>,
        temporal_queries_details: Option<&[ExecutionResultDetails]>,
    ) -> Result<u64, AndromedaError> {
        let result = sqlx::query(
            "UPDATE snapshots SET status = $2::snapshot_status, queries_details = $3, temporal_queries_details = $4, modified_at = NOW() \
             WHERE id = $1 AND status = 'preparing'",
        )
        .bind(id.as_str())
        .bind(<&str>::from(status))
        .bind(queries_details.and_then(|d| serde_json::to_value(d).ok()))
        .bind(temporal_queries_details.and_then(|d| serde_json::to_value(d).ok()))
        .execute(self.pool())
        .await?;
        Ok(result.rows_affected())
    }

    async fn delete(&self, id: &SnapshotId) -> Result<u64, AndromedaError> {
        let result = sqlx::query("DELETE FROM snapshots WHERE id = $1")
            .bind(id.as_str())
            .execute(self.pool())
            .await?;
        Ok(result.rows_affected())
    }

    async fn match_by_filter(&self, filter: &SnapshotPurgeFilter) -> Result<Vec<SnapshotId>, AndromedaError> {
        let built = build_match_query(filter);
        let query = andromeda_postgresql_query::bind_values!(sqlx::query(&built.sql), &built.bind_values);
        let rows = query.fetch_all(self.pool()).await?;
        let mut ids = Vec::with_capacity(rows.len());
        for row in &rows {
            let raw: String = row.try_get("id")?;
            let parsed = raw
                .parse::<SnapshotId>()
                .map_err(|e| AndromedaError::internal(format!("Invalid snapshot id '{raw}' in snapshots table: {e}")))?;
            ids.push(parsed);
        }
        Ok(ids)
    }

    async fn delete_one(&self, id: &SnapshotId) -> Result<u64, AndromedaError> {
        let result = sqlx::query("DELETE FROM snapshots WHERE id = $1")
            .bind(id.as_str())
            .execute(self.pool())
            .await?;
        Ok(result.rows_affected())
    }

    async fn clone_snapshot(&self, source_id: &SnapshotId, new_id: &SnapshotId, new_expires_at: DateTime<Utc>) -> Result<Snapshot, AndromedaError> {
        let mut tx = self.pool().begin().await?;

        sqlx::query(
            "INSERT INTO snapshots (id, snapshot_queries, snapshot_temporal_queries, snapshot_lifetime, snapshot_priority, endpoint, receiver_info, status, queries_details, temporal_queries_details, created_at, modified_at, expires_at, last_used_at)
             SELECT $2, snapshot_queries, snapshot_temporal_queries, snapshot_lifetime, snapshot_priority, endpoint, receiver_info, status, queries_details, temporal_queries_details, NOW(), NOW(), $3, NULL
             FROM snapshots WHERE id = $1",
        )
        .bind(source_id.as_str())
        .bind(new_id.as_str())
        .bind(new_expires_at)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "INSERT INTO snapshot_entities (snapshot_id, entity_id, entity_data)
             SELECT $2, entity_id, entity_data FROM snapshot_entities WHERE snapshot_id = $1",
        )
        .bind(source_id.as_str())
        .bind(new_id.as_str())
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "INSERT INTO snapshot_temporal_entities (snapshot_id, entity_id, temporal_data)
             SELECT $2, entity_id, temporal_data FROM snapshot_temporal_entities WHERE snapshot_id = $1",
        )
        .bind(source_id.as_str())
        .bind(new_id.as_str())
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        self.get(new_id)
            .await?
            .ok_or_else(|| AndromedaError::snapshot_not_found("Cloned snapshot not found after insert".to_string()))
    }

    async fn delete_expired(&self) -> Result<u64, AndromedaError> {
        let result = sqlx::query("DELETE FROM snapshots WHERE expires_at < NOW()").execute(self.pool()).await?;
        Ok(result.rows_affected())
    }

    async fn touch_last_used(&self, id: &SnapshotId) -> Result<(), AndromedaError> {
        sqlx::query("UPDATE snapshots SET last_used_at = NOW() WHERE id = $1")
            .bind(id.as_str())
            .execute(self.pool())
            .await?;
        Ok(())
    }

    async fn insert_entities(&self, snapshot_id: &SnapshotId, entities: &[SnapshotEntityInput<'_>]) -> Result<(), AndromedaError> {
        batch_insert_snapshot_rows(self.pool(), "snapshot_entities", "entity_data", snapshot_id, entities).await
    }

    async fn insert_temporal_entities(&self, snapshot_id: &SnapshotId, entities: &[SnapshotEntityInput<'_>]) -> Result<(), AndromedaError> {
        batch_insert_snapshot_rows(self.pool(), "snapshot_temporal_entities", "temporal_data", snapshot_id, entities).await
    }

    async fn get_entity(&self, snapshot_id: &SnapshotId, entity_id: &Uri) -> Result<Option<Value>, AndromedaError> {
        get_snapshot_row(self.pool(), "snapshot_entities", "entity_data", snapshot_id, entity_id).await
    }

    async fn query_entities(
        &self,
        snapshot_id: &SnapshotId,
        entity_type: Option<&str>,
        entity_ids: Option<&[Uri]>,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<Value>, i64), AndromedaError> {
        query_snapshot_table(&SnapshotQueryParams {
            pool: self.pool(),
            table: "snapshot_entities",
            data_column: "entity_data",
            snapshot_id,
            entity_type,
            entity_ids,
            limit,
            offset,
        })
        .await
    }

    async fn get_temporal_entity(&self, snapshot_id: &SnapshotId, entity_id: &Uri) -> Result<Option<Value>, AndromedaError> {
        get_snapshot_row(self.pool(), "snapshot_temporal_entities", "temporal_data", snapshot_id, entity_id).await
    }

    async fn query_temporal_entities(
        &self,
        snapshot_id: &SnapshotId,
        entity_type: Option<&str>,
        entity_ids: Option<&[Uri]>,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<Value>, i64), AndromedaError> {
        query_snapshot_table(&SnapshotQueryParams {
            pool: self.pool(),
            table: "snapshot_temporal_entities",
            data_column: "temporal_data",
            snapshot_id,
            entity_type,
            entity_ids,
            limit,
            offset,
        })
        .await
    }

    async fn snapshot_distinct_types(&self, snapshot_id: &SnapshotId) -> Result<Vec<String>, AndromedaError> {
        let rows = sqlx::query("SELECT DISTINCT entity_data->>'type' AS t FROM snapshot_entities WHERE snapshot_id = $1 AND entity_data ? 'type' ORDER BY t")
            .bind(snapshot_id.as_str())
            .fetch_all(self.pool())
            .await?;
        let mut types = Vec::with_capacity(rows.len());
        for row in &rows {
            if let Ok(Some(t)) = row.try_get::<Option<String>, _>("t") {
                types.push(t);
            }
        }
        Ok(types)
    }

    async fn snapshot_type_info(&self, snapshot_id: &SnapshotId, type_name: &str) -> Result<Option<(i64, Vec<String>)>, AndromedaError> {
        let count_row = sqlx::query("SELECT COUNT(*) AS cnt FROM snapshot_entities WHERE snapshot_id = $1 AND entity_data->>'type' = $2")
            .bind(snapshot_id.as_str())
            .bind(type_name)
            .fetch_one(self.pool())
            .await?;
        let count: i64 = count_row.try_get("cnt")?;
        if count == 0 {
            return Ok(None);
        }

        let attr_rows = sqlx::query(
            "SELECT DISTINCT key FROM snapshot_entities, jsonb_object_keys(entity_data) AS key \
             WHERE snapshot_id = $1 AND entity_data->>'type' = $2 AND key NOT IN ('id', 'type', '@context') \
             ORDER BY key",
        )
        .bind(snapshot_id.as_str())
        .bind(type_name)
        .fetch_all(self.pool())
        .await?;
        let mut attrs = Vec::with_capacity(attr_rows.len());
        for row in &attr_rows {
            let key: String = row.try_get("key")?;
            attrs.push(key);
        }
        Ok(Some((count, attrs)))
    }

    async fn snapshot_distinct_attributes(&self, snapshot_id: &SnapshotId) -> Result<Vec<String>, AndromedaError> {
        let rows = sqlx::query(
            "SELECT DISTINCT key FROM snapshot_entities, jsonb_object_keys(entity_data) AS key \
             WHERE snapshot_id = $1 AND key NOT IN ('id', 'type', '@context') ORDER BY key",
        )
        .bind(snapshot_id.as_str())
        .fetch_all(self.pool())
        .await?;
        let mut attrs = Vec::with_capacity(rows.len());
        for row in &rows {
            let key: String = row.try_get("key")?;
            attrs.push(key);
        }
        Ok(attrs)
    }

    async fn snapshot_attribute_info(&self, snapshot_id: &SnapshotId, attr_name: &str) -> Result<Option<(i64, Vec<String>)>, AndromedaError> {
        let count_row = sqlx::query("SELECT COUNT(*) AS cnt FROM snapshot_entities WHERE snapshot_id = $1 AND entity_data ? $2")
            .bind(snapshot_id.as_str())
            .bind(attr_name)
            .fetch_one(self.pool())
            .await?;
        let count: i64 = count_row.try_get("cnt")?;
        if count == 0 {
            return Ok(None);
        }

        let type_rows = sqlx::query(
            "SELECT DISTINCT entity_data->>'type' AS t FROM snapshot_entities \
             WHERE snapshot_id = $1 AND entity_data ? $2 AND entity_data ? 'type' ORDER BY t",
        )
        .bind(snapshot_id.as_str())
        .bind(attr_name)
        .fetch_all(self.pool())
        .await?;
        let mut types = Vec::with_capacity(type_rows.len());
        for row in &type_rows {
            if let Ok(Some(t)) = row.try_get::<Option<String>, _>("t") {
                types.push(t);
            }
        }
        Ok(Some((count, types)))
    }
}

async fn query_snapshot_table(params: &SnapshotQueryParams<'_>) -> Result<(Vec<Value>, i64), AndromedaError> {
    let built = build_listing_query(&SnapshotListingParams {
        table: params.table,
        data_column: params.data_column,
        snapshot_id: params.snapshot_id.as_str(),
        entity_type: params.entity_type,
        entity_ids: params.entity_ids,
        limit: params.limit,
        offset: params.offset,
    });
    let Some(count_sql) = built.count_sql.as_deref() else {
        return Err(AndromedaError::internal("snapshot listing query must carry a count statement"));
    };

    let count_row = andromeda_postgresql_query::bind_values!(sqlx::query(count_sql), &built.bind_values)
        .fetch_one(params.pool)
        .await?;
    let total: i64 = count_row.try_get("cnt")?;

    let rows = andromeda_postgresql_query::bind_values!(sqlx::query(&built.sql), &built.bind_values)
        .fetch_all(params.pool)
        .await?;
    let entities: Vec<Value> = rows.iter().map(|r| r.try_get(params.data_column)).collect::<Result<Vec<_>, _>>()?;

    Ok((entities, total))
}

async fn batch_insert_snapshot_rows(
    pool: &PgPool,
    table: &str,
    data_column: &str,
    snapshot_id: &SnapshotId,
    entities: &[SnapshotEntityInput<'_>],
) -> Result<(), AndromedaError> {
    if entities.is_empty() {
        return Ok(());
    }
    let entity_ids: Vec<&str> = entities.iter().map(|e| e.entity_id).collect();
    let payloads: Vec<&Value> = entities.iter().map(|e| e.data).collect();

    let sql = format!(
        "INSERT INTO {table} (snapshot_id, entity_id, {data_column}) \
         SELECT $1, d.entity_id, d.data \
         FROM UNNEST($2::text[], $3::jsonb[]) AS d(entity_id, data) \
         ON CONFLICT (snapshot_id, entity_id) DO UPDATE SET {data_column} = EXCLUDED.{data_column}"
    );
    sqlx::query(&sql)
        .bind(snapshot_id.as_str())
        .bind(&entity_ids)
        .bind(&payloads)
        .execute(pool)
        .await?;
    Ok(())
}

async fn get_snapshot_row(pool: &PgPool, table: &str, data_column: &str, snapshot_id: &SnapshotId, entity_id: &Uri) -> Result<Option<Value>, AndromedaError> {
    let built = build_get_row_query(Alias::new(table), Alias::new(data_column), snapshot_id, entity_id);
    let row = andromeda_postgresql_query::bind_values!(sqlx::query(&built.sql), &built.bind_values)
        .fetch_optional(pool)
        .await?;

    match row {
        Some(row) => {
            let data: Value = row.try_get(data_column)?;
            Ok(Some(data))
        }
        None => Ok(None),
    }
}
