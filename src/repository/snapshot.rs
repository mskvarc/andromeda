use crate::domain::snapshot::{ExecutionResultDetails, Snapshot, SnapshotStatus};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{Row, postgres::PgPool};

pub struct SnapshotRepository {
    pool: PgPool,
}

impl SnapshotRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn create(&self, snapshot: &Snapshot) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO snapshots (id, snapshot_queries, snapshot_temporal_queries, snapshot_lifetime, snapshot_priority, endpoint, receiver_info, status, queries_details, temporal_queries_details, created_at, modified_at, expires_at, last_used_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)"
        )
        .bind(&snapshot.id)
        .bind(serde_json::to_value(&snapshot.snapshot_queries).ok())
        .bind(serde_json::to_value(&snapshot.snapshot_temporal_queries).ok())
        .bind(&snapshot.snapshot_lifetime)
        .bind(snapshot.snapshot_priority)
        .bind(&snapshot.endpoint)
        .bind(serde_json::to_value(&snapshot.receiver_info).ok())
        .bind(snapshot.status.to_string())
        .bind(serde_json::to_value(&snapshot.queries_details).ok())
        .bind(serde_json::to_value(&snapshot.temporal_queries_details).ok())
        .bind(snapshot.created_at)
        .bind(snapshot.modified_at)
        .bind(snapshot.expires_at)
        .bind(snapshot.last_used_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

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

    pub async fn exists(&self, id: &str) -> Result<bool, sqlx::Error> {
        let row = sqlx::query("SELECT 1 FROM snapshots WHERE id = $1").bind(id).fetch_optional(&self.pool).await?;
        Ok(row.is_some())
    }

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

    pub async fn delete(&self, id: &str) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM snapshots WHERE id = $1").bind(id).execute(&self.pool).await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn delete_all(&self) -> Result<u64, sqlx::Error> {
        let result = sqlx::query("DELETE FROM snapshots").execute(&self.pool).await?;
        Ok(result.rows_affected())
    }

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

    pub async fn touch_last_used(&self, id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE snapshots SET last_used_at = NOW() WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
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
