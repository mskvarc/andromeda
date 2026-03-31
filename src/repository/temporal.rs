//! Repository layer for the NGSI-LD Temporal API.
//!
//! Handles TEA (temporal_entity_attributes) catalog and attribute_history operations.

use crate::{domain::temporal::TeaEntry, error::AndromedaError, query::sql::BindValue};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::postgres::PgPool;

pub struct HistoryValues<'a> {
    pub value_number: Option<f64>,
    pub value_string: Option<&'a str>,
    pub value_bool: Option<bool>,
    pub value_geo: Option<&'a str>,
    pub value_object: Option<&'a Value>,
    pub value_datetime: Option<DateTime<Utc>>,
    pub value_uri: Option<&'a str>,
    pub metadata: Option<&'a Value>,
}

pub struct InsertHistoryParams<'a> {
    pub time: DateTime<Utc>,
    pub tea_id: uuid::Uuid,
    pub values: HistoryValues<'a>,
    pub instance_id: &'a str,
    pub observed_at: Option<DateTime<Utc>>,
}

pub struct UpdateInstanceParams<'a> {
    pub instance_id: &'a str,
    pub entity_id: &'a str,
    pub attribute_name: &'a str,
    pub values: HistoryValues<'a>,
    pub observed_at: Option<DateTime<Utc>>,
}

pub struct TemporalRepository {
    pool: PgPool,
}

impl TemporalRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
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
    /// - `delete_all`: delete all instances of the attribute
    /// - `dataset_id: None` with `delete_all: false`: delete only default instance (dataset_id IS NULL)
    /// - `dataset_id: Some(s)`: delete the instance with that datasetId
    pub async fn delete_tea(&self, entity_id: &str, attribute_name: &str, dataset_id: Option<&str>, delete_all: bool) -> Result<(), AndromedaError> {
        if delete_all {
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
        } else if let Some(ds_id) = dataset_id {
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

        Ok(())
    }

    // -------------------------------------------------------------------------
    // Attribute History Operations
    // -------------------------------------------------------------------------

    /// Insert one observation into attribute_history.
    /// Only one value column should be non-NULL, determined by the TEA's value_type.
    pub async fn insert_history(&self, params: InsertHistoryParams<'_>) -> Result<(), AndromedaError> {
        let mut tx = self.pool.begin().await?;

        // Remove any existing row with the same (tea_id, instance_id) to avoid
        // duplicates when an entity update re-records the same attribute instance.
        sqlx::query("DELETE FROM attribute_history WHERE tea_id = $1 AND instance_id = $2")
            .bind(params.tea_id)
            .bind(params.instance_id)
            .execute(&mut *tx)
            .await?;

        // Also remove any existing row with the same (tea_id, observed_at) to handle
        // temporal entity updates where the same time instant gets a new instance_id.
        // Per NGSI-LD spec, create_or_update should merge/replace instances at the same time.
        if let Some(obs) = params.observed_at {
            sqlx::query("DELETE FROM attribute_history WHERE tea_id = $1 AND observed_at = $2")
                .bind(params.tea_id)
                .bind(obs)
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
    pub async fn execute_temporal_query(&self, sql: &str, binds: &[BindValue]) -> Result<Vec<sqlx::postgres::PgRow>, AndromedaError> {
        let mut query = sqlx::query(sql);
        for bind in binds {
            query = match bind {
                BindValue::String(s) => query.bind(s),
                BindValue::StringVec(v) => query.bind(v),
                BindValue::I64(i) => query.bind(i),
            };
        }

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
            return Err(AndromedaError::not_found(format!(
                "Attribute instance {} not found",
                params.instance_id
            )));
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
