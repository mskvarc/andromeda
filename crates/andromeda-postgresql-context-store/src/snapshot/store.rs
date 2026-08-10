//! `SnapshotStore` trait implementation for `SnapshotRepository`.

use crate::snapshot::{mapper::row_to_snapshot, params::SnapshotQueryParams, repo::SnapshotRepository};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    snapshot::{ExecutionResultDetails, Snapshot, SnapshotPriority, SnapshotStatus},
    snapshot_filter::SnapshotPurgeFilter,
};
use andromeda_ngsi_ld_query::type_selection::grammar::TypeExprRaw;
use andromeda_postgresql_context_query::{
    snapshot_listing::{SnapshotListingParams, build_get_row_query, build_listing_query},
    snapshot_purge::build_match_query,
};
use andromeda_snapshot::{
    error::SnapshotError,
    store::{InsertSnapshotParams, SnapshotAttributeInfo, SnapshotEntityInput, SnapshotStore, SnapshotTypeInfo},
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use iso8601_duration::Duration as Iso8601Duration;
use sea_query::Alias;
use serde_json::Value;
use sqlx::{Row, postgres::PgPool};
use std::fmt::Display;
use url::Url;

/// Erase an adapter failure into the snapshot policy's persistence category.
fn persistence_error(error: impl Display) -> SnapshotError {
    SnapshotError::persistence(error.to_string())
}

#[async_trait]
impl SnapshotStore for SnapshotRepository {
    async fn create(&self, params: InsertSnapshotParams<'_>) -> Result<(), SnapshotError> {
        sqlx::query(
            "INSERT INTO snapshots (id, snapshot_queries, snapshot_temporal_queries, snapshot_lifetime, snapshot_priority, endpoint, receiver_info, status, queries_details, temporal_queries_details, created_at, modified_at, expires_at, last_used_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8::snapshot_status, $9, $10, $11, $12, $13, $14)",
        )
        .bind(params.id.as_str())
        .bind(params.snapshot_queries.map(serde_json::to_value).transpose()?)
        .bind(params.snapshot_temporal_queries.map(serde_json::to_value).transpose()?)
        .bind(params.snapshot_lifetime.map(ToString::to_string))
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
        .await.map_err(persistence_error)?;
        Ok(())
    }

    async fn get(&self, id: &Uri) -> Result<Option<Snapshot>, SnapshotError> {
        let row = sqlx::query(
            "SELECT id, snapshot_queries, snapshot_temporal_queries, snapshot_lifetime, snapshot_priority, endpoint, receiver_info, status::text AS status, queries_details, temporal_queries_details, created_at, modified_at, expires_at, last_used_at FROM snapshots WHERE id = $1",
        )
        .bind(id.as_str())
        .fetch_optional(self.pool())
        .await.map_err(persistence_error)?;

        match row {
            Some(row) => Ok(Some(row_to_snapshot(&row).map_err(persistence_error)?)),
            None => Ok(None),
        }
    }

    async fn exists(&self, id: &Uri) -> Result<bool, SnapshotError> {
        let row = sqlx::query("SELECT 1 FROM snapshots WHERE id = $1")
            .bind(id.as_str())
            .fetch_optional(self.pool())
            .await
            .map_err(persistence_error)?;
        Ok(row.is_some())
    }

    async fn next_preparing(&self) -> Result<Option<Snapshot>, SnapshotError> {
        let row = sqlx::query(
            "SELECT id, snapshot_queries, snapshot_temporal_queries, snapshot_lifetime, snapshot_priority, endpoint, receiver_info, status::text AS status, queries_details, temporal_queries_details, created_at, modified_at, expires_at, last_used_at FROM snapshots WHERE status = 'preparing' ORDER BY snapshot_priority DESC, created_at ASC LIMIT 1",
        )
        .fetch_optional(self.pool())
        .await
        .map_err(persistence_error)?;
        row.as_ref().map(row_to_snapshot).transpose().map_err(persistence_error)
    }

    async fn update_lifetime_priority(
        &self,
        id: &Uri,
        lifetime: Option<&Iso8601Duration>,
        priority: Option<SnapshotPriority>,
        new_expires_at: Option<DateTime<Utc>>,
    ) -> Result<u64, SnapshotError> {
        let result = sqlx::query(
            "UPDATE snapshots SET snapshot_lifetime = COALESCE($2, snapshot_lifetime), snapshot_priority = COALESCE($3, snapshot_priority), expires_at = COALESCE($4, expires_at), modified_at = NOW() WHERE id = $1",
        )
        .bind(id.as_str())
        .bind(lifetime.map(ToString::to_string))
        .bind(priority.map(|p| i16::from(p.as_u8())))
        .bind(new_expires_at)
        .execute(self.pool())
        .await.map_err(persistence_error)?;
        Ok(result.rows_affected())
    }

    async fn update_status(
        &self,
        id: &Uri,
        status: SnapshotStatus,
        queries_details: Option<&[ExecutionResultDetails]>,
        temporal_queries_details: Option<&[ExecutionResultDetails]>,
    ) -> Result<u64, SnapshotError> {
        let result = sqlx::query(
            "UPDATE snapshots SET status = $2::snapshot_status, queries_details = $3, temporal_queries_details = $4, modified_at = NOW() \
             WHERE id = $1 AND status = 'preparing'",
        )
        .bind(id.as_str())
        .bind(<&str>::from(status))
        .bind(queries_details.and_then(|d| serde_json::to_value(d).ok()))
        .bind(temporal_queries_details.and_then(|d| serde_json::to_value(d).ok()))
        .execute(self.pool())
        .await
        .map_err(persistence_error)?;
        Ok(result.rows_affected())
    }

    async fn delete(&self, id: &Uri) -> Result<u64, SnapshotError> {
        let result = sqlx::query("DELETE FROM snapshots WHERE id = $1")
            .bind(id.as_str())
            .execute(self.pool())
            .await
            .map_err(persistence_error)?;
        Ok(result.rows_affected())
    }

    async fn match_by_filter(&self, filter: &SnapshotPurgeFilter) -> Result<Vec<Uri>, SnapshotError> {
        let built = build_match_query(filter);
        let query = andromeda_postgresql_query::bind_values!(sqlx::query(&built.sql), &built.bind_values);
        let rows = query.fetch_all(self.pool()).await.map_err(persistence_error)?;
        let mut ids = Vec::with_capacity(rows.len());
        for row in &rows {
            let raw: String = row.try_get("id").map_err(persistence_error)?;
            let parsed = raw
                .parse::<Uri>()
                .map_err(|e| SnapshotError::internal(format!("Invalid snapshot id '{raw}' in snapshots table: {e}")))?;
            ids.push(parsed);
        }
        Ok(ids)
    }

    async fn delete_one(&self, id: &Uri) -> Result<u64, SnapshotError> {
        let result = sqlx::query("DELETE FROM snapshots WHERE id = $1")
            .bind(id.as_str())
            .execute(self.pool())
            .await
            .map_err(persistence_error)?;
        Ok(result.rows_affected())
    }

    async fn clone_snapshot(&self, source_id: &Uri, new_id: &Uri, new_expires_at: DateTime<Utc>) -> Result<Snapshot, SnapshotError> {
        let mut tx = self.pool().begin().await.map_err(persistence_error)?;

        sqlx::query(
            "INSERT INTO snapshots (id, snapshot_queries, snapshot_temporal_queries, snapshot_lifetime, snapshot_priority, endpoint, receiver_info, status, queries_details, temporal_queries_details, created_at, modified_at, expires_at, last_used_at)
             SELECT $2, snapshot_queries, snapshot_temporal_queries, snapshot_lifetime, snapshot_priority, endpoint, receiver_info, status, queries_details, temporal_queries_details, NOW(), NOW(), $3, NULL
             FROM snapshots WHERE id = $1",
        )
        .bind(source_id.as_str())
        .bind(new_id.as_str())
        .bind(new_expires_at)
        .execute(&mut *tx)
        .await.map_err(persistence_error)?;

        sqlx::query(
            "INSERT INTO snapshot_entities (snapshot_id, entity_id, entity_data)
             SELECT $2, entity_id, entity_data FROM snapshot_entities WHERE snapshot_id = $1",
        )
        .bind(source_id.as_str())
        .bind(new_id.as_str())
        .execute(&mut *tx)
        .await
        .map_err(persistence_error)?;

        sqlx::query(
            "INSERT INTO snapshot_temporal_entities (snapshot_id, entity_id, temporal_data)
             SELECT $2, entity_id, temporal_data FROM snapshot_temporal_entities WHERE snapshot_id = $1",
        )
        .bind(source_id.as_str())
        .bind(new_id.as_str())
        .execute(&mut *tx)
        .await
        .map_err(persistence_error)?;

        tx.commit().await.map_err(persistence_error)?;

        self.get(new_id).await?.ok_or_else(|| SnapshotError::not_found(new_id))
    }

    async fn delete_expired(&self, cutoff: DateTime<Utc>) -> Result<u64, SnapshotError> {
        let result = sqlx::query("DELETE FROM snapshots WHERE expires_at <= $1")
            .bind(cutoff)
            .execute(self.pool())
            .await
            .map_err(persistence_error)?;
        Ok(result.rows_affected())
    }

    async fn touch_last_used(&self, id: &Uri) -> Result<(), SnapshotError> {
        sqlx::query("UPDATE snapshots SET last_used_at = NOW() WHERE id = $1")
            .bind(id.as_str())
            .execute(self.pool())
            .await
            .map_err(persistence_error)?;
        Ok(())
    }

    async fn insert_entities(&self, snapshot_id: &Uri, entities: &[SnapshotEntityInput<'_>]) -> Result<(), SnapshotError> {
        batch_insert_snapshot_rows(self.pool(), "snapshot_entities", "entity_data", snapshot_id, entities).await
    }

    async fn insert_temporal_entities(&self, snapshot_id: &Uri, entities: &[SnapshotEntityInput<'_>]) -> Result<(), SnapshotError> {
        batch_insert_snapshot_rows(self.pool(), "snapshot_temporal_entities", "temporal_data", snapshot_id, entities).await
    }

    async fn get_entity(&self, snapshot_id: &Uri, entity_id: &Uri) -> Result<Option<Value>, SnapshotError> {
        get_snapshot_row(self.pool(), "snapshot_entities", "entity_data", snapshot_id, entity_id).await
    }

    async fn query_entities(
        &self,
        snapshot_id: &Uri,
        entity_type: Option<&TypeExprRaw>,
        entity_ids: Option<&[Uri]>,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<Value>, i64), SnapshotError> {
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

    async fn get_temporal_entity(&self, snapshot_id: &Uri, entity_id: &Uri) -> Result<Option<Value>, SnapshotError> {
        get_snapshot_row(self.pool(), "snapshot_temporal_entities", "temporal_data", snapshot_id, entity_id).await
    }

    async fn query_temporal_entities(
        &self,
        snapshot_id: &Uri,
        entity_type: Option<&TypeExprRaw>,
        entity_ids: Option<&[Uri]>,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<Value>, i64), SnapshotError> {
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

    async fn snapshot_distinct_types(&self, snapshot_id: &Uri) -> Result<Vec<EntityTypeName>, SnapshotError> {
        let rows = sqlx::query("SELECT DISTINCT entity_data->>'type' AS t FROM snapshot_entities WHERE snapshot_id = $1 AND entity_data ? 'type' ORDER BY t")
            .bind(snapshot_id.as_str())
            .fetch_all(self.pool())
            .await
            .map_err(persistence_error)?;
        let mut types = Vec::with_capacity(rows.len());
        for row in &rows {
            if let Some(raw) = row.try_get::<Option<String>, _>("t").map_err(persistence_error)? {
                types.push(parse_snapshot_entity_type(raw)?);
            }
        }
        Ok(types)
    }

    async fn snapshot_type_info(&self, snapshot_id: &Uri, type_name: &EntityTypeName) -> Result<Option<SnapshotTypeInfo>, SnapshotError> {
        let count_row = sqlx::query("SELECT COUNT(*) AS cnt FROM snapshot_entities WHERE snapshot_id = $1 AND entity_data->>'type' = $2")
            .bind(snapshot_id.as_str())
            .bind(type_name.as_str())
            .fetch_one(self.pool())
            .await
            .map_err(persistence_error)?;
        let count: i64 = count_row.try_get("cnt").map_err(persistence_error)?;
        if count == 0 {
            return Ok(None);
        }

        let attr_rows = sqlx::query(
            "SELECT DISTINCT key FROM snapshot_entities, jsonb_object_keys(entity_data) AS key \
             WHERE snapshot_id = $1 AND entity_data->>'type' = $2 AND key NOT IN ('id', 'type', '@context') \
             ORDER BY key",
        )
        .bind(snapshot_id.as_str())
        .bind(type_name.as_str())
        .fetch_all(self.pool())
        .await
        .map_err(persistence_error)?;
        let mut attrs = Vec::with_capacity(attr_rows.len());
        for row in &attr_rows {
            let raw: String = row.try_get("key").map_err(persistence_error)?;
            attrs.push(parse_snapshot_attribute_name(raw)?);
        }
        Ok(Some((count, attrs)))
    }

    async fn snapshot_distinct_attributes(&self, snapshot_id: &Uri) -> Result<Vec<AttributeName>, SnapshotError> {
        let rows = sqlx::query(
            "SELECT DISTINCT key FROM snapshot_entities, jsonb_object_keys(entity_data) AS key \
             WHERE snapshot_id = $1 AND key NOT IN ('id', 'type', '@context') ORDER BY key",
        )
        .bind(snapshot_id.as_str())
        .fetch_all(self.pool())
        .await
        .map_err(persistence_error)?;
        let mut attrs = Vec::with_capacity(rows.len());
        for row in &rows {
            let raw: String = row.try_get("key").map_err(persistence_error)?;
            attrs.push(parse_snapshot_attribute_name(raw)?);
        }
        Ok(attrs)
    }

    async fn snapshot_attribute_info(&self, snapshot_id: &Uri, attr_name: &AttributeName) -> Result<Option<SnapshotAttributeInfo>, SnapshotError> {
        let count_row = sqlx::query("SELECT COUNT(*) AS cnt FROM snapshot_entities WHERE snapshot_id = $1 AND entity_data ? $2")
            .bind(snapshot_id.as_str())
            .bind(attr_name.as_str())
            .fetch_one(self.pool())
            .await
            .map_err(persistence_error)?;
        let count: i64 = count_row.try_get("cnt").map_err(persistence_error)?;
        if count == 0 {
            return Ok(None);
        }

        let type_rows = sqlx::query(
            "SELECT DISTINCT entity_data->>'type' AS t FROM snapshot_entities \
             WHERE snapshot_id = $1 AND entity_data ? $2 AND entity_data ? 'type' ORDER BY t",
        )
        .bind(snapshot_id.as_str())
        .bind(attr_name.as_str())
        .fetch_all(self.pool())
        .await
        .map_err(persistence_error)?;
        let mut types = Vec::with_capacity(type_rows.len());
        for row in &type_rows {
            if let Some(raw) = row.try_get::<Option<String>, _>("t").map_err(persistence_error)? {
                types.push(parse_snapshot_entity_type(raw)?);
            }
        }
        Ok(Some((count, types)))
    }
}

/// Query one snapshot backing table with the common selector and pagination contract.
async fn query_snapshot_table(params: &SnapshotQueryParams<'_>) -> Result<(Vec<Value>, i64), SnapshotError> {
    let built = build_listing_query(&SnapshotListingParams {
        table: params.table,
        data_column: params.data_column,
        snapshot_id: params.snapshot_id,
        entity_type: params.entity_type,
        entity_ids: params.entity_ids,
        limit: params.limit,
        offset: params.offset,
    });
    let Some(count_sql) = built.count_sql.as_deref() else {
        return Err(SnapshotError::internal("snapshot listing query must carry a count statement"));
    };

    let count_row = andromeda_postgresql_query::bind_values!(sqlx::query(count_sql), &built.bind_values)
        .fetch_one(params.pool)
        .await
        .map_err(persistence_error)?;
    let total: i64 = count_row.try_get("cnt").map_err(persistence_error)?;

    let rows = andromeda_postgresql_query::bind_values!(sqlx::query(&built.sql), &built.bind_values)
        .fetch_all(params.pool)
        .await
        .map_err(persistence_error)?;
    let entities: Vec<Value> = rows
        .iter()
        .map(|r| r.try_get(params.data_column))
        .collect::<Result<Vec<_>, _>>()
        .map_err(persistence_error)?;

    Ok((entities, total))
}

/// Parse one entity type decoded from a snapshot projection.
fn parse_snapshot_entity_type(raw: String) -> Result<EntityTypeName, SnapshotError> {
    raw.parse::<EntityTypeName>()
        .map_err(|error| SnapshotError::internal(format!("invalid snapshot entity type '{raw}' in database: {error}")))
}

/// Parse one attribute name decoded from a snapshot projection.
fn parse_snapshot_attribute_name(raw: String) -> Result<AttributeName, SnapshotError> {
    raw.parse::<AttributeName>()
        .map_err(|error| SnapshotError::internal(format!("invalid snapshot attribute name '{raw}' in database: {error}")))
}

/// Insert prepared snapshot rows in bounded PostgreSQL batches.
async fn batch_insert_snapshot_rows(
    pool: &PgPool,
    table: &str,
    data_column: &str,
    snapshot_id: &Uri,
    entities: &[SnapshotEntityInput<'_>],
) -> Result<(), SnapshotError> {
    if entities.is_empty() {
        return Ok(());
    }
    let entity_ids: Vec<&str> = entities.iter().map(|entity| entity.entity_id.as_str()).collect();
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
        .await
        .map_err(persistence_error)?;
    Ok(())
}

/// Retrieve one prepared snapshot row by snapshot and entity identifiers.
async fn get_snapshot_row(pool: &PgPool, table: &str, data_column: &str, snapshot_id: &Uri, entity_id: &Uri) -> Result<Option<Value>, SnapshotError> {
    let built = build_get_row_query(Alias::new(table), Alias::new(data_column), snapshot_id, entity_id);
    let row = andromeda_postgresql_query::bind_values!(sqlx::query(&built.sql), &built.bind_values)
        .fetch_optional(pool)
        .await
        .map_err(persistence_error)?;

    match row {
        Some(row) => {
            let data: Value = row.try_get(data_column).map_err(persistence_error)?;
            Ok(Some(data))
        }
        None => Ok(None),
    }
}
