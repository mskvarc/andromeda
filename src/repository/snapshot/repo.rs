//! SQL-only repository for snapshot records.

use crate::{
    core::error::AndromedaError,
    domain::{
        ngsi_ld_urn::SnapshotId,
        snapshot::{ExecutionResultDetails, Snapshot, SnapshotPriority, SnapshotStatus},
        snapshot_filter::{Conjunction, SnapshotPurgeFilter, SnapshotValue},
    },
    repository::snapshot::{
        mapper::row_to_snapshot,
        params::{InsertSnapshotParams, SnapshotEntityInput, SnapshotQueryParams},
    },
};
use chrono::{DateTime, Utc};
use iso8601_duration::Duration as Iso8601Duration;
use serde_json::Value;
use sqlx::{QueryBuilder, Row, postgres::PgPool};
use url::Url;

/// Repository for persisting and querying snapshot records.
#[derive(Debug)]
pub struct SnapshotRepository {
    pool: PgPool,
}

impl SnapshotRepository {
    /// Create a new snapshot repository backed by the given connection pool.
    pub fn new(pool: &PgPool) -> SnapshotRepository {
        SnapshotRepository { pool: pool.clone() } // clone: Arc ref-count bump
    }

    /// Insert a new snapshot. The service layer pre-serialises JSON columns
    /// into [`InsertSnapshotParams`] so no `to_value` is needed here.
    pub async fn create(&self, params: InsertSnapshotParams<'_>) -> Result<(), AndromedaError> {
        sqlx::query(
            "INSERT INTO snapshots (id, snapshot_queries, snapshot_temporal_queries, snapshot_lifetime, snapshot_priority, endpoint, receiver_info, status, queries_details, temporal_queries_details, created_at, modified_at, expires_at, last_used_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)",
        )
        .bind(params.id.as_str())
        .bind(params.snapshot_queries_json)
        .bind(params.snapshot_temporal_queries_json)
        .bind(params.snapshot_lifetime)
        .bind(params.snapshot_priority)
        .bind(params.endpoint.map(Url::as_str))
        .bind(params.receiver_info_json)
        .bind(params.status)
        .bind(params.queries_details_json)
        .bind(params.temporal_queries_details_json)
        .bind(params.created_at)
        .bind(params.modified_at)
        .bind(params.expires_at)
        .bind(params.last_used_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Retrieve a snapshot by its identifier.
    pub async fn get(&self, id: &SnapshotId) -> Result<Option<Snapshot>, AndromedaError> {
        let row = sqlx::query(
            "SELECT id, snapshot_queries, snapshot_temporal_queries, snapshot_lifetime, snapshot_priority, endpoint, receiver_info, status, queries_details, temporal_queries_details, created_at, modified_at, expires_at, last_used_at FROM snapshots WHERE id = $1",
        )
        .bind(id.as_str())
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => Ok(Some(row_to_snapshot(&row)?)),
            None => Ok(None),
        }
    }

    /// Check whether a snapshot with the given identifier exists.
    pub async fn exists(&self, id: &SnapshotId) -> Result<bool, AndromedaError> {
        let row = sqlx::query("SELECT 1 FROM snapshots WHERE id = $1")
            .bind(id.as_str())
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.is_some())
    }

    /// Update the lifetime, priority, and expiration of an existing snapshot. Returns rows affected.
    pub async fn update_lifetime_priority(
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
        .bind(priority.map(SnapshotPriority::as_i16))
        .bind(new_expires_at)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    /// Delete a snapshot by its identifier. Returns rows affected.
    pub async fn delete(&self, id: &SnapshotId) -> Result<u64, AndromedaError> {
        let result = sqlx::query("DELETE FROM snapshots WHERE id = $1").bind(id.as_str()).execute(&self.pool).await?;
        Ok(result.rows_affected())
    }

    /// Identifiers of all snapshots matching the given purge filter.
    pub async fn match_by_filter(&self, filter: &SnapshotPurgeFilter) -> Result<Vec<SnapshotId>, AndromedaError> {
        let separator = match filter.conjunction {
            Conjunction::And => " AND ",
            Conjunction::Or => " OR ",
        };

        let mut qb: QueryBuilder<sqlx::Postgres> = QueryBuilder::new("SELECT id FROM snapshots WHERE ");
        let mut first = true;
        for term in &filter.terms {
            if !first {
                qb.push(separator);
            }
            first = false;
            qb.push(term.field.sql_column());
            qb.push(" ");
            qb.push(term.op.sql());
            qb.push(" ");
            match &term.value {
                SnapshotValue::Id(id) => {
                    qb.push_bind(id.as_str().to_string());
                }
                SnapshotValue::Priority(priority) => {
                    qb.push_bind(priority.as_i16());
                }
                SnapshotValue::Status(status) => {
                    qb.push_bind(*status);
                }
                SnapshotValue::Endpoint(url) => {
                    qb.push_bind(url.as_str().to_string());
                }
                SnapshotValue::DateTime(dt) => {
                    qb.push_bind(*dt);
                }
            }
        }

        let rows = qb.build().fetch_all(&self.pool).await?;
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

    /// Delete a single snapshot by identifier. Returns rows affected.
    pub async fn delete_one(&self, id: &SnapshotId) -> Result<u64, AndromedaError> {
        let result = sqlx::query("DELETE FROM snapshots WHERE id = $1").bind(id.as_str()).execute(&self.pool).await?;
        Ok(result.rows_affected())
    }

    /// Duplicate a snapshot under a new identifier and expiration.
    pub async fn clone_snapshot(&self, source_id: &SnapshotId, new_id: &SnapshotId, new_expires_at: DateTime<Utc>) -> Result<Snapshot, AndromedaError> {
        let mut tx = self.pool.begin().await?;

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

    /// Update a snapshot's status and execution details after query execution.
    ///
    /// The service layer is responsible for validating the state transition
    /// (see `service::snapshot::status::validate_preparing_transition`); this
    /// method issues an unconditional UPDATE guarded only by the current DB
    /// status being `preparing`.
    pub async fn update_status(
        &self,
        id: &SnapshotId,
        status: SnapshotStatus,
        queries_details: Option<&[ExecutionResultDetails]>,
        temporal_queries_details: Option<&[ExecutionResultDetails]>,
    ) -> Result<u64, AndromedaError> {
        let result = sqlx::query(
            "UPDATE snapshots SET status = $2, queries_details = $3, temporal_queries_details = $4, modified_at = NOW() \
             WHERE id = $1 AND status = 'preparing'",
        )
        .bind(id.as_str())
        .bind(status)
        .bind(queries_details.and_then(|d| serde_json::to_value(d).ok()))
        .bind(temporal_queries_details.and_then(|d| serde_json::to_value(d).ok()))
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    /// Batch insert entities into `snapshot_entities` in a single UNNEST-based
    /// INSERT (one round-trip per batch, vs one per entity previously).
    pub async fn insert_entities(&self, snapshot_id: &SnapshotId, entities: &[SnapshotEntityInput<'_>]) -> Result<(), AndromedaError> {
        batch_insert_snapshot_rows(&self.pool, "snapshot_entities", "entity_data", snapshot_id, entities).await
    }

    /// Batch insert temporal entities into `snapshot_temporal_entities` via
    /// UNNEST.
    pub async fn insert_temporal_entities(&self, snapshot_id: &SnapshotId, entities: &[SnapshotEntityInput<'_>]) -> Result<(), AndromedaError> {
        batch_insert_snapshot_rows(&self.pool, "snapshot_temporal_entities", "temporal_data", snapshot_id, entities).await
    }

    /// Delete all expired snapshots and return the count removed.
    pub async fn delete_expired(&self) -> Result<u64, AndromedaError> {
        let result = sqlx::query("DELETE FROM snapshots WHERE expires_at < NOW()").execute(&self.pool).await?;
        Ok(result.rows_affected())
    }

    /// Update the `last_used_at` timestamp to the current time.
    pub async fn touch_last_used(&self, id: &SnapshotId) -> Result<(), AndromedaError> {
        sqlx::query("UPDATE snapshots SET last_used_at = NOW() WHERE id = $1")
            .bind(id.as_str())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Retrieve a single entity from `snapshot_entities`.
    pub async fn get_entity(&self, snapshot_id: &SnapshotId, entity_id: &str) -> Result<Option<Value>, AndromedaError> {
        get_snapshot_row(&self.pool, "snapshot_entities", "entity_data", snapshot_id, entity_id).await
    }

    /// Query entities with optional type and ID filtering.
    pub async fn query_entities(
        &self,
        snapshot_id: &SnapshotId,
        entity_type: Option<&str>,
        entity_ids: Option<&[String]>,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<Value>, i64), AndromedaError> {
        query_snapshot_table(&SnapshotQueryParams {
            pool: &self.pool,
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

    /// Retrieve a single temporal entity.
    pub async fn get_temporal_entity(&self, snapshot_id: &SnapshotId, entity_id: &str) -> Result<Option<Value>, AndromedaError> {
        get_snapshot_row(&self.pool, "snapshot_temporal_entities", "temporal_data", snapshot_id, entity_id).await
    }

    /// Distinct `type` values in a snapshot.
    pub async fn snapshot_distinct_types(&self, snapshot_id: &SnapshotId) -> Result<Vec<String>, AndromedaError> {
        let rows = sqlx::query("SELECT DISTINCT entity_data->>'type' AS t FROM snapshot_entities WHERE snapshot_id = $1 AND entity_data ? 'type' ORDER BY t")
            .bind(snapshot_id.as_str())
            .fetch_all(&self.pool)
            .await?;
        let mut types = Vec::with_capacity(rows.len());
        for row in &rows {
            if let Ok(Some(t)) = row.try_get::<Option<String>, _>("t") {
                types.push(t);
            }
        }
        Ok(types)
    }

    /// Type info: entity count and attribute names for a given type.
    pub async fn snapshot_type_info(&self, snapshot_id: &SnapshotId, type_name: &str) -> Result<Option<(i64, Vec<String>)>, AndromedaError> {
        let count_row = sqlx::query("SELECT COUNT(*) AS cnt FROM snapshot_entities WHERE snapshot_id = $1 AND entity_data->>'type' = $2")
            .bind(snapshot_id.as_str())
            .bind(type_name)
            .fetch_one(&self.pool)
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
        .fetch_all(&self.pool)
        .await?;
        let mut attrs = Vec::with_capacity(attr_rows.len());
        for row in &attr_rows {
            let key: String = row.try_get("key")?;
            attrs.push(key);
        }
        Ok(Some((count, attrs)))
    }

    /// Distinct attribute names observed in a snapshot.
    pub async fn snapshot_distinct_attributes(&self, snapshot_id: &SnapshotId) -> Result<Vec<String>, AndromedaError> {
        let rows = sqlx::query(
            "SELECT DISTINCT key FROM snapshot_entities, jsonb_object_keys(entity_data) AS key \
             WHERE snapshot_id = $1 AND key NOT IN ('id', 'type', '@context') ORDER BY key",
        )
        .bind(snapshot_id.as_str())
        .fetch_all(&self.pool)
        .await?;
        let mut attrs = Vec::with_capacity(rows.len());
        for row in &rows {
            let key: String = row.try_get("key")?;
            attrs.push(key);
        }
        Ok(attrs)
    }

    /// Attribute info: entity count and types for a given attribute.
    pub async fn snapshot_attribute_info(&self, snapshot_id: &SnapshotId, attr_name: &str) -> Result<Option<(i64, Vec<String>)>, AndromedaError> {
        let count_row = sqlx::query("SELECT COUNT(*) AS cnt FROM snapshot_entities WHERE snapshot_id = $1 AND entity_data ? $2")
            .bind(snapshot_id.as_str())
            .bind(attr_name)
            .fetch_one(&self.pool)
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
        .fetch_all(&self.pool)
        .await?;
        let mut types = Vec::with_capacity(type_rows.len());
        for row in &type_rows {
            if let Ok(Some(t)) = row.try_get::<Option<String>, _>("t") {
                types.push(t);
            }
        }
        Ok(Some((count, types)))
    }

    /// Query temporal entities with optional filtering.
    pub async fn query_temporal_entities(
        &self,
        snapshot_id: &SnapshotId,
        entity_type: Option<&str>,
        entity_ids: Option<&[String]>,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<Value>, i64), AndromedaError> {
        query_snapshot_table(&SnapshotQueryParams {
            pool: &self.pool,
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
}

async fn query_snapshot_table(params: &SnapshotQueryParams<'_>) -> Result<(Vec<Value>, i64), AndromedaError> {
    use sea_query::{Alias, Asterisk, Expr, Order, PostgresQueryBuilder, Query};
    use sea_query_binder::SqlxBinder;

    let SnapshotQueryParams {
        pool,
        table,
        data_column,
        snapshot_id,
        entity_type,
        entity_ids,
        limit,
        offset,
    } = params;

    let table_ident = Alias::new(*table);
    let data_ident = Alias::new(*data_column);
    let snapshot_col = Alias::new("snapshot_id");
    let entity_id_col = Alias::new("entity_id");

    let apply_where = |q: &mut sea_query::SelectStatement| {
        q.and_where(Expr::col(snapshot_col.clone()).eq(snapshot_id.as_str()));
        if let Some(t) = entity_type {
            let fragment = format!("{}->>'type' LIKE '%' || $1 || '%'", data_column);
            q.and_where(Expr::cust_with_values(&fragment, [*t]));
        }
        if let Some(ids) = entity_ids
            && !ids.is_empty()
        {
            let owned: Vec<String> = ids.to_vec(); // clone: sea_query requires owned values
            q.and_where(Expr::col(entity_id_col.clone()).is_in(owned));
        }
    };

    let mut count_builder = Query::select();
    count_builder
        .expr_as(Expr::count(Expr::col(Asterisk)), Alias::new("cnt"))
        .from(table_ident.clone());
    apply_where(&mut count_builder);
    let (count_sql, count_values) = count_builder.build_sqlx(PostgresQueryBuilder);
    let count_row = sqlx::query_with(&count_sql, count_values).fetch_one(*pool).await?;
    let total: i64 = count_row.try_get("cnt")?;

    let mut data_builder = Query::select();
    data_builder.column(data_ident.clone()).from(table_ident);
    apply_where(&mut data_builder);
    data_builder
        .order_by(entity_id_col, Order::Asc)
        .limit(u64::try_from(*limit).unwrap_or(0))
        .offset(u64::try_from(*offset).unwrap_or(0));
    let (data_sql, data_values) = data_builder.build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(&data_sql, data_values).fetch_all(*pool).await?;
    let entities: Vec<Value> = rows.iter().map(|r| r.try_get(*data_column)).collect::<Result<Vec<_>, _>>()?;

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

async fn get_snapshot_row(pool: &PgPool, table: &str, data_column: &str, snapshot_id: &SnapshotId, entity_id: &str) -> Result<Option<Value>, AndromedaError> {
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query};
    use sea_query_binder::SqlxBinder;

    let mut select = Query::select();
    select
        .column(Alias::new(data_column))
        .from(Alias::new(table))
        .and_where(Expr::col(Alias::new("snapshot_id")).eq(snapshot_id.as_str()))
        .and_where(Expr::col(Alias::new("entity_id")).eq(entity_id));
    let (sql, values) = select.build_sqlx(PostgresQueryBuilder);
    let row = sqlx::query_with(&sql, values).fetch_optional(pool).await?;

    match row {
        Some(row) => {
            let data: Value = row.try_get(data_column)?;
            Ok(Some(data))
        }
        None => Ok(None),
    }
}
