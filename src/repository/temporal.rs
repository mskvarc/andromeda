//! Repository layer for the NGSI-LD Temporal API.
//!
//! Handles TEA (temporal_entity_attributes) catalog and attribute_history operations.

use crate::{domain::temporal::TeaEntry, error::AndromedaError, query::sql::BindValue};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::postgres::PgPool;

/// Whether to delete all instances of an attribute or a single specific instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteScope {
    /// Delete all instances of the attribute regardless of dataset ID.
    All,
    /// Delete only the instance matching the specific dataset ID (or the default if `None`).
    Single,
}

/// Typed value columns for an attribute history row (exactly one should be set).
#[derive(Debug)]
pub struct HistoryValues<'a> {
    /// Numeric attribute value.
    pub value_number: Option<f64>,
    /// String attribute value.
    pub value_string: Option<&'a str>,
    /// Boolean attribute value.
    pub value_bool: Option<bool>,
    /// GeoJSON geometry as a serialized string.
    pub value_geo: Option<&'a str>,
    /// JSON object attribute value.
    pub value_object: Option<&'a Value>,
    /// DateTime attribute value.
    pub value_datetime: Option<DateTime<Utc>>,
    /// URI attribute value.
    pub value_uri: Option<&'a str>,
    /// Additional metadata associated with the attribute instance.
    pub metadata: Option<&'a Value>,
}

/// Parameters for inserting a single observation into attribute history.
#[derive(Debug)]
pub struct InsertHistoryParams<'a> {
    /// Timestamp used for the TimescaleDB partition key.
    pub time: DateTime<Utc>,
    /// Foreign key referencing the temporal entity attribute catalog entry.
    pub tea_id: uuid::Uuid,
    /// Typed value columns for this observation.
    pub values: HistoryValues<'a>,
    /// Unique identifier for this attribute instance.
    pub instance_id: &'a str,
    /// Application-level observation timestamp, if provided.
    pub observed_at: Option<DateTime<Utc>>,
}

/// Parameters for modifying an existing attribute instance in history.
#[derive(Debug)]
pub struct UpdateInstanceParams<'a> {
    /// Identifier of the attribute instance to update.
    pub instance_id: &'a str,
    /// Entity that owns the attribute.
    pub entity_id: &'a str,
    /// Expanded attribute name.
    pub attribute_name: &'a str,
    /// New typed value columns for this instance.
    pub values: HistoryValues<'a>,
    /// Updated observation timestamp, if provided.
    pub observed_at: Option<DateTime<Utc>>,
}

/// PostgreSQL/TimescaleDB-backed repository for NGSI-LD temporal entity data.
#[derive(Debug)]
pub struct TemporalRepository {
    pool: PgPool,
}

impl TemporalRepository {
    /// Creates a new repository backed by the given connection pool.
    pub fn new(pool: &PgPool) -> TemporalRepository {
        TemporalRepository { pool: pool.clone() } // clone: Arc ref-count bump (PgPool is Arc-based)
    }

    // -------------------------------------------------------------------------
    // TEA (Temporal Entity Attributes) Operations
    // -------------------------------------------------------------------------

    /// Find or create a catalog entry, returns UUID.
    pub async fn upsert_tea(
        &self,
        entity_id: &str,
        attribute_name: &str,
        attribute_type: &str,
        dataset_id: Option<&str>,
        value_type: &str,
    ) -> Result<uuid::Uuid, AndromedaError> {
        let id: uuid::Uuid = sqlx::query_scalar(
            r#"
            INSERT INTO temporal_entity_attributes (entity_id, attribute_name, attribute_type, dataset_id, value_type)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (entity_id, attribute_name, dataset_id) DO UPDATE
            SET attribute_type = EXCLUDED.attribute_type, value_type = EXCLUDED.value_type
            RETURNING id
            "#,
        )
        .bind(entity_id)
        .bind(attribute_name)
        .bind(attribute_type)
        .bind(dataset_id)
        .bind(value_type)
        .fetch_one(&self.pool)
        .await?;

        Ok(id)
    }

    /// TEA entries for a specific attribute.
    pub async fn get_teas_for_entity_attr(&self, entity_id: &str, attribute_name: &str) -> Result<Vec<TeaEntry>, AndromedaError> {
        let rows = sqlx::query_as(
            r#"
            SELECT id, entity_id, attribute_name, attribute_type, dataset_id, value_type
            FROM temporal_entity_attributes
            WHERE entity_id = $1 AND attribute_name = $2
            "#,
        )
        .bind(entity_id)
        .bind(attribute_name)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }

    /// Check if any temporal data exists for an entity.
    pub async fn tea_exists_for_entity(&self, entity_id: &str) -> Result<bool, AndromedaError> {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM temporal_entity_attributes WHERE entity_id = $1)")
            .bind(entity_id)
            .fetch_one(&self.pool)
            .await?;

        Ok(exists)
    }

    /// Delete all TEAs for an entity (cascades to history).
    pub async fn delete_teas_for_entity(&self, entity_id: &str) -> Result<(), AndromedaError> {
        sqlx::query("DELETE FROM temporal_entity_attributes WHERE entity_id = $1")
            .bind(entity_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    /// Delete specific TEA entry.
    /// - `scope: DeleteScope::All`: delete all instances of the attribute
    /// - `scope: DeleteScope::Single` with `dataset_id: None`: delete only default instance (dataset_id IS NULL)
    /// - `scope: DeleteScope::Single` with `dataset_id: Some(s)`: delete the instance with that datasetId
    pub async fn delete_tea(&self, entity_id: &str, attribute_name: &str, dataset_id: Option<&str>, scope: DeleteScope) -> Result<(), AndromedaError> {
        match scope {
            DeleteScope::All => {
                sqlx::query(
                    r#"
                    DELETE FROM temporal_entity_attributes
                    WHERE entity_id = $1 AND attribute_name = $2
                    "#,
                )
                .bind(entity_id)
                .bind(attribute_name)
                .execute(&self.pool)
                .await?;
            }
            DeleteScope::Single => {
                if let Some(ds_id) = dataset_id {
                    sqlx::query(
                        r#"
                        DELETE FROM temporal_entity_attributes
                        WHERE entity_id = $1 AND attribute_name = $2
                        AND dataset_id = $3
                        "#,
                    )
                    .bind(entity_id)
                    .bind(attribute_name)
                    .bind(ds_id)
                    .execute(&self.pool)
                    .await?;
                } else {
                    sqlx::query(
                        r#"
                        DELETE FROM temporal_entity_attributes
                        WHERE entity_id = $1 AND attribute_name = $2 AND dataset_id IS NULL
                        "#,
                    )
                    .bind(entity_id)
                    .bind(attribute_name)
                    .execute(&self.pool)
                    .await?;
                }
            }
        }

        Ok(())
    }

    // -------------------------------------------------------------------------
    // Attribute History Operations
    // -------------------------------------------------------------------------

    /// Insert one observation into attribute_history.
    /// Only one value column should be non-NULL, determined by the TEA's value_type.
    ///
    /// Deduplication: removes conflicting rows before insertion. Two dedup axes:
    ///   1. Same (tea_id, instance_id) — entity update re-records the same attribute instance
    ///   2. Same (tea_id, observed_at) — temporal create_or_update merges at the same timestamp
    ///
    /// Note: INSERT ON CONFLICT is not viable here because TimescaleDB requires
    /// the partitioning column (`time`) in all unique constraints, but dedup keys
    /// do not include `time` (instance_id may appear at a new timestamp, observed_at
    /// may differ from `time`).
    pub async fn insert_history(&self, params: InsertHistoryParams<'_>) -> Result<(), AndromedaError> {
        let mut tx = self.pool.begin().await?;

        // Single DELETE covering both dedup axes in one round-trip.
        if let Some(obs) = params.observed_at {
            sqlx::query("DELETE FROM attribute_history WHERE tea_id = $1 AND (instance_id = $2 OR observed_at = $3)")
                .bind(params.tea_id)
                .bind(params.instance_id)
                .bind(obs)
                .execute(&mut *tx)
                .await?;
        } else {
            sqlx::query("DELETE FROM attribute_history WHERE tea_id = $1 AND instance_id = $2")
                .bind(params.tea_id)
                .bind(params.instance_id)
                .execute(&mut *tx)
                .await?;
        }

        sqlx::query(
            r#"
            INSERT INTO attribute_history
                (time, tea_id, value_number, value_string, value_bool, value_geo, value_object, value_datetime, value_uri, instance_id, observed_at, metadata)
            VALUES
                ($1, $2, $3, $4, $5, ST_SetSRID(ST_GeomFromGeoJSON($6), 4326), $7, $8, $9, $10, $11, $12)
            "#,
        )
        .bind(params.time)
        .bind(params.tea_id)
        .bind(params.values.value_number)
        .bind(params.values.value_string)
        .bind(params.values.value_bool)
        .bind(params.values.value_geo)
        .bind(params.values.value_object)
        .bind(params.values.value_datetime)
        .bind(params.values.value_uri)
        .bind(params.instance_id)
        .bind(params.observed_at)
        .bind(params.values.metadata)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(())
    }

    /// Execute a dynamically-built temporal query. Returns raw rows for the service to map.
    pub async fn execute_temporal_query(&self, sql: &str, binds: &[BindValue<'_>]) -> Result<Vec<sqlx::postgres::PgRow>, AndromedaError> {
        let query = crate::bind_values!(sqlx::query(sql), binds);

        let rows = query.fetch_all(&self.pool).await?;
        Ok(rows)
    }

    /// Delete a specific attribute instance. Returns 404 if no row was deleted.
    pub async fn delete_instance(&self, instance_id: &str, entity_id: &str, attribute_name: &str) -> Result<(), AndromedaError> {
        let result = sqlx::query(
            r#"
            DELETE FROM attribute_history
            WHERE instance_id = $1
            AND tea_id IN (
                SELECT id FROM temporal_entity_attributes
                WHERE entity_id = $2 AND attribute_name = $3
            )
            RETURNING instance_id
            "#,
        )
        .bind(instance_id)
        .bind(entity_id)
        .bind(attribute_name)
        .fetch_optional(&self.pool)
        .await?;

        if result.is_none() {
            return Err(AndromedaError::not_found(format!("Attribute instance {} not found", instance_id)));
        }

        Ok(())
    }

    /// Modify a specific attribute instance.
    pub async fn update_instance(&self, params: UpdateInstanceParams<'_>) -> Result<(), AndromedaError> {
        let result = sqlx::query(
            r#"
            UPDATE attribute_history
            SET value_number = $4, value_string = $5, value_bool = $6,
                value_geo = ST_SetSRID(ST_GeomFromGeoJSON($7), 4326),
                value_object = $8, value_datetime = $9, value_uri = $10,
                observed_at = $11, metadata = $12
            WHERE instance_id = $1
            AND tea_id IN (
                SELECT id FROM temporal_entity_attributes
                WHERE entity_id = $2 AND attribute_name = $3
            )
            RETURNING instance_id
            "#,
        )
        .bind(params.instance_id)
        .bind(params.entity_id)
        .bind(params.attribute_name)
        .bind(params.values.value_number)
        .bind(params.values.value_string)
        .bind(params.values.value_bool)
        .bind(params.values.value_geo)
        .bind(params.values.value_object)
        .bind(params.values.value_datetime)
        .bind(params.values.value_uri)
        .bind(params.observed_at)
        .bind(params.values.metadata)
        .fetch_optional(&self.pool)
        .await?;

        if result.is_none() {
            return Err(AndromedaError::not_found(format!("Attribute instance {} not found", params.instance_id)));
        }

        Ok(())
    }

    /// Fetch a single instance for validation. Returns the row if found.
    pub async fn get_instance(&self, instance_id: &str, entity_id: &str, attribute_name: &str) -> Result<Option<sqlx::postgres::PgRow>, AndromedaError> {
        let row = sqlx::query(
            r#"
            SELECT ah.*, tea.attribute_name, tea.attribute_type, tea.value_type
            FROM attribute_history ah
            JOIN temporal_entity_attributes tea ON tea.id = ah.tea_id
            WHERE ah.instance_id = $1
            AND tea.entity_id = $2 AND tea.attribute_name = $3
            "#,
        )
        .bind(instance_id)
        .bind(entity_id)
        .bind(attribute_name)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row)
    }
}
