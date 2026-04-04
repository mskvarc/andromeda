use crate::domain::snapshot::{ExecutionResultDetails, Snapshot, SnapshotStatus};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{Row, postgres::PgPool};

/// Repository for persisting and querying snapshot records.
pub struct SnapshotRepository {
    pool: PgPool,
}

impl SnapshotRepository {
    /// Create a new snapshot repository backed by the given connection pool.
    pub fn new(pool: &PgPool) -> SnapshotRepository {
        SnapshotRepository { pool: pool.clone() } // clone: Arc ref-count bump (PgPool is Arc-based)
    }

    /// Insert a new snapshot into the database.
    pub async fn create(&self, snapshot: &Snapshot) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO snapshots (id, snapshot_queries, snapshot_temporal_queries, snapshot_lifetime, snapshot_priority, endpoint, receiver_info, status, queries_details, temporal_queries_details, created_at, modified_at, expires_at, last_used_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)",
        )
        .bind(&snapshot.id)
        .bind(snapshot.snapshot_queries.as_ref().and_then(|v| serde_json::to_value(v).ok()))
        .bind(snapshot.snapshot_temporal_queries.as_ref().and_then(|v| serde_json::to_value(v).ok()))
        .bind(&snapshot.snapshot_lifetime)
        .bind(snapshot.snapshot_priority)
        .bind(&snapshot.endpoint)
        .bind(snapshot.receiver_info.as_ref().and_then(|v| serde_json::to_value(v).ok()))
        .bind(snapshot.status.to_string())
        .bind(snapshot.queries_details.as_ref().and_then(|v| serde_json::to_value(v).ok()))
        .bind(snapshot.temporal_queries_details.as_ref().and_then(|v| serde_json::to_value(v).ok()))
        .bind(snapshot.created_at)
        .bind(snapshot.modified_at)
        .bind(snapshot.expires_at)
        .bind(snapshot.last_used_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Retrieve a snapshot by its identifier, returning `None` if not found.
    pub async fn get(&self, id: &str) -> Result<Option<Snapshot>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT id, snapshot_queries, snapshot_temporal_queries, snapshot_lifetime, snapshot_priority, endpoint, receiver_info, status, queries_details, temporal_queries_details, created_at, modified_at, expires_at, last_used_at FROM snapshots WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => Ok(Some(row_to_snapshot(&row)?)),
            None => Ok(None),
        }
    }

    /// Check whether a snapshot with the given identifier exists.
    pub async fn exists(&self, id: &str) -> Result<bool, sqlx::Error> {
        let row = sqlx::query("SELECT 1 FROM snapshots WHERE id = $1").bind(id).fetch_optional(&self.pool).await?;
        Ok(row.is_some())
    }

    /// Update the lifetime, priority, and expiration of an existing snapshot.
    pub async fn update_lifetime_priority(
        &self,
        id: &str,
        lifetime: Option<&str>,
        priority: Option<i16>,
        new_expires_at: Option<DateTime<Utc>>,
    ) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE snapshots SET snapshot_lifetime = COALESCE($2, snapshot_lifetime), snapshot_priority = COALESCE($3, snapshot_priority), expires_at = COALESCE($4, expires_at), modified_at = NOW() WHERE id = $1",
        )
        .bind(id)
        .bind(lifetime)
        .bind(priority)
        .bind(new_expires_at)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Delete a snapshot by its identifier. Returns `true` if a row was removed.
    pub async fn delete(&self, id: &str) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM snapshots WHERE id = $1").bind(id).execute(&self.pool).await?;
        Ok(result.rows_affected() > 0)
    }

    /// Delete all snapshots and return the number of rows removed.
    pub async fn delete_all(&self) -> Result<u64, sqlx::Error> {
        let result = sqlx::query("DELETE FROM snapshots").execute(&self.pool).await?;
        Ok(result.rows_affected())
    }

    /// Duplicate a snapshot (metadata, entities, and temporal data) under a new identifier and expiration.
    pub async fn clone_snapshot(&self, source_id: &str, new_id: &str, new_expires_at: DateTime<Utc>) -> Result<Snapshot, sqlx::Error> {
        // Copy the snapshot metadata
        sqlx::query(
            "INSERT INTO snapshots (id, snapshot_queries, snapshot_temporal_queries, snapshot_lifetime, snapshot_priority, endpoint, receiver_info, status, queries_details, temporal_queries_details, created_at, modified_at, expires_at, last_used_at)
             SELECT $2, snapshot_queries, snapshot_temporal_queries, snapshot_lifetime, snapshot_priority, endpoint, receiver_info, status, queries_details, temporal_queries_details, NOW(), NOW(), $3, NULL
             FROM snapshots WHERE id = $1",
        )
        .bind(source_id)
        .bind(new_id)
        .bind(new_expires_at)
        .execute(&self.pool)
        .await?;

        // Copy entity data
        sqlx::query(
            "INSERT INTO snapshot_entities (snapshot_id, entity_id, entity_data)
             SELECT $2, entity_id, entity_data FROM snapshot_entities WHERE snapshot_id = $1",
        )
        .bind(source_id)
        .bind(new_id)
        .execute(&self.pool)
        .await?;

        // Copy temporal data
        sqlx::query(
            "INSERT INTO snapshot_temporal_entities (snapshot_id, entity_id, temporal_data)
             SELECT $2, entity_id, temporal_data FROM snapshot_temporal_entities WHERE snapshot_id = $1",
        )
        .bind(source_id)
        .bind(new_id)
        .execute(&self.pool)
        .await?;

        self.get(new_id).await?.ok_or_else(|| sqlx::Error::RowNotFound)
    }

    /// Update the `last_used_at` timestamp to the current time.
    pub async fn touch_last_used(&self, id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE snapshots SET last_used_at = NOW() WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Retrieve a single entity from `snapshot_entities` by snapshot and entity identifiers.
    pub async fn get_entity(&self, snapshot_id: &str, entity_id: &str) -> Result<Option<Value>, sqlx::Error> {
        let row = sqlx::query("SELECT entity_data FROM snapshot_entities WHERE snapshot_id = $1 AND entity_id = $2")
            .bind(snapshot_id)
            .bind(entity_id)
            .fetch_optional(&self.pool)
            .await?;

        match row {
            Some(row) => {
                let data: Value = row.try_get("entity_data")?;
                Ok(Some(data))
            }
            None => Ok(None),
        }
    }

    /// Query entities from `snapshot_entities` with optional type and ID filtering.
    ///
    /// Returns `(entities, total_count)` for pagination support.
    pub async fn query_entities(
        &self,
        snapshot_id: &str,
        entity_type: Option<&str>,
        entity_ids: Option<&[String]>,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<Value>, i64), sqlx::Error> {
        // Build a dynamic query with optional filters
        let mut conditions = vec!["snapshot_id = $1".to_string()];
        let mut param_idx = 2u32;

        if entity_type.is_some() {
            conditions.push(format!("entity_data->>'type' LIKE '%' || ${param_idx} || '%'"));
            param_idx += 1;
        }

        if let Some(ids) = entity_ids {
            let placeholders: Vec<String> = ids.iter().enumerate().map(|(i, _)| format!("${}", param_idx + i as u32)).collect();
            if !placeholders.is_empty() {
                conditions.push(format!("entity_id IN ({})", placeholders.join(", ")));
            }
            param_idx += ids.len() as u32;
        }

        let where_clause = conditions.join(" AND ");

        // Count query
        let count_sql = format!("SELECT COUNT(*) as cnt FROM snapshot_entities WHERE {where_clause}");
        let mut count_query = sqlx::query(&count_sql).bind(snapshot_id);
        if let Some(t) = entity_type {
            count_query = count_query.bind(t);
        }
        if let Some(ids) = entity_ids {
            for id in ids {
                count_query = count_query.bind(id);
            }
        }
        let count_row = count_query.fetch_one(&self.pool).await?;
        let total: i64 = count_row.try_get("cnt")?;

        // Data query
        let data_sql = format!(
            "SELECT entity_data FROM snapshot_entities WHERE {where_clause} ORDER BY entity_id LIMIT ${param_idx} OFFSET ${}",
            param_idx + 1
        );
        let mut data_query = sqlx::query(&data_sql).bind(snapshot_id);
        if let Some(t) = entity_type {
            data_query = data_query.bind(t);
        }
        if let Some(ids) = entity_ids {
            for id in ids {
                data_query = data_query.bind(id);
            }
        }
        data_query = data_query.bind(limit).bind(offset);

        let rows = data_query.fetch_all(&self.pool).await?;
        let entities: Vec<Value> = rows.iter().map(|r| r.try_get("entity_data")).collect::<Result<Vec<_>, _>>()?;

        Ok((entities, total))
    }

    /// Retrieve a single temporal entity from `snapshot_temporal_entities`.
    pub async fn get_temporal_entity(&self, snapshot_id: &str, entity_id: &str) -> Result<Option<Value>, sqlx::Error> {
        let row = sqlx::query("SELECT temporal_data FROM snapshot_temporal_entities WHERE snapshot_id = $1 AND entity_id = $2")
            .bind(snapshot_id)
            .bind(entity_id)
            .fetch_optional(&self.pool)
            .await?;

        match row {
            Some(row) => {
                let data: Value = row.try_get("temporal_data")?;
                Ok(Some(data))
            }
            None => Ok(None),
        }
    }

    /// Query temporal entities from `snapshot_temporal_entities` with optional filtering.
    ///
    /// Returns `(entities, total_count)` for pagination support.
    pub async fn query_temporal_entities(
        &self,
        snapshot_id: &str,
        entity_type: Option<&str>,
        entity_ids: Option<&[String]>,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<Value>, i64), sqlx::Error> {
        let mut conditions = vec!["snapshot_id = $1".to_string()];
        let mut param_idx = 2u32;

        if entity_type.is_some() {
            conditions.push(format!("temporal_data->>'type' LIKE '%' || ${param_idx} || '%'"));
            param_idx += 1;
        }

        if let Some(ids) = entity_ids {
            let placeholders: Vec<String> = ids.iter().enumerate().map(|(i, _)| format!("${}", param_idx + i as u32)).collect();
            if !placeholders.is_empty() {
                conditions.push(format!("entity_id IN ({})", placeholders.join(", ")));
            }
            param_idx += ids.len() as u32;
        }

        let where_clause = conditions.join(" AND ");

        let count_sql = format!("SELECT COUNT(*) as cnt FROM snapshot_temporal_entities WHERE {where_clause}");
        let mut count_query = sqlx::query(&count_sql).bind(snapshot_id);
        if let Some(t) = entity_type {
            count_query = count_query.bind(t);
        }
        if let Some(ids) = entity_ids {
            for id in ids {
                count_query = count_query.bind(id);
            }
        }
        let count_row = count_query.fetch_one(&self.pool).await?;
        let total: i64 = count_row.try_get("cnt")?;

        let data_sql = format!(
            "SELECT temporal_data FROM snapshot_temporal_entities WHERE {where_clause} ORDER BY entity_id LIMIT ${param_idx} OFFSET ${}",
            param_idx + 1
        );
        let mut data_query = sqlx::query(&data_sql).bind(snapshot_id);
        if let Some(t) = entity_type {
            data_query = data_query.bind(t);
        }
        if let Some(ids) = entity_ids {
            for id in ids {
                data_query = data_query.bind(id);
            }
        }
        data_query = data_query.bind(limit).bind(offset);

        let rows = data_query.fetch_all(&self.pool).await?;
        let entities: Vec<Value> = rows.iter().map(|r| r.try_get("temporal_data")).collect::<Result<Vec<_>, _>>()?;

        Ok((entities, total))
    }
}

fn row_to_snapshot(row: &sqlx::postgres::PgRow) -> Result<Snapshot, sqlx::Error> {
    let status_str: String = row.try_get("status")?;
    let status = status_str.parse::<SnapshotStatus>().map_err(|e| sqlx::Error::ColumnDecode {
        index: "status".to_string(),
        source: Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Invalid snapshot status '{}': {}", status_str, e),
        )),
    })?;

    let queries_details_val: Option<Value> = row.try_get("queries_details")?;
    let queries_details: Option<Vec<ExecutionResultDetails>> = queries_details_val
        .map(|v| {
            serde_json::from_value(v).map_err(|e| sqlx::Error::ColumnDecode {
                index: "queries_details".to_string(),
                source: Box::new(e),
            })
        })
        .transpose()?;

    let temporal_queries_details_val: Option<Value> = row.try_get("temporal_queries_details")?;
    let temporal_queries_details: Option<Vec<ExecutionResultDetails>> = temporal_queries_details_val
        .map(|v| {
            serde_json::from_value(v).map_err(|e| sqlx::Error::ColumnDecode {
                index: "temporal_queries_details".to_string(),
                source: Box::new(e),
            })
        })
        .transpose()?;

    let snapshot_queries_val: Option<Value> = row.try_get("snapshot_queries")?;
    let snapshot_queries: Option<Vec<Value>> = snapshot_queries_val
        .map(|v| {
            serde_json::from_value(v).map_err(|e| sqlx::Error::ColumnDecode {
                index: "snapshot_queries".to_string(),
                source: Box::new(e),
            })
        })
        .transpose()?;

    let snapshot_temporal_queries_val: Option<Value> = row.try_get("snapshot_temporal_queries")?;
    let snapshot_temporal_queries: Option<Vec<Value>> = snapshot_temporal_queries_val
        .map(|v| {
            serde_json::from_value(v).map_err(|e| sqlx::Error::ColumnDecode {
                index: "snapshot_temporal_queries".to_string(),
                source: Box::new(e),
            })
        })
        .transpose()?;

    let receiver_info_val: Option<Value> = row.try_get("receiver_info")?;
    let receiver_info: Option<Vec<Value>> = receiver_info_val
        .map(|v| {
            serde_json::from_value(v).map_err(|e| sqlx::Error::ColumnDecode {
                index: "receiver_info".to_string(),
                source: Box::new(e),
            })
        })
        .transpose()?;

    Ok(Snapshot {
        id: row.try_get("id")?,
        snapshot_queries,
        snapshot_temporal_queries,
        snapshot_lifetime: row.try_get("snapshot_lifetime")?,
        snapshot_priority: row.try_get("snapshot_priority")?,
        endpoint: row.try_get("endpoint")?,
        receiver_info,
        status,
        queries_details,
        temporal_queries_details,
        created_at: row.try_get("created_at")?,
        modified_at: row.try_get("modified_at")?,
        expires_at: row.try_get("expires_at")?,
        last_used_at: row.try_get("last_used_at")?,
    })
}
