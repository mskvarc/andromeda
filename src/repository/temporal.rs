//! Repository layer for the NGSI-LD Temporal API.
//!
//! Handles TEA (temporal_entity_attributes) catalog and attribute_history operations.

use crate::{
    core::error::AndromedaError,
    domain::{
        attribute_type::AttributeType,
        temporal::{TeaEntry, ValueType},
    },
    query::sql::BindValue,
};
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use iref::IriBuf;
use serde_json::Value;
use sqlx::{
    Row,
    postgres::{PgPool, PgRow},
};
use std::{collections::HashMap, str::FromStr, sync::Arc};
use urn::Urn;

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
    /// In-process cache for attribute name dictionary lookups.
    ///
    /// With only ~30 distinct attribute names, caching avoids repeated
    /// database round-trips on every batch operation. New names are inserted
    /// on first encounter and cached for the process lifetime.
    attr_name_cache: Arc<DashMap<String, i16>>,
}

impl TemporalRepository {
    /// Creates a new repository backed by the given connection pool.
    pub fn new(pool: &PgPool) -> TemporalRepository {
        TemporalRepository {
            pool: pool.clone(), // clone: Arc ref-count bump (PgPool is Arc-based)
            attr_name_cache: Arc::new(DashMap::new()),
        }
    }

    // -------------------------------------------------------------------------
    // Attribute name dictionary
    // -------------------------------------------------------------------------

    /// Resolve a single attribute name to its dictionary ID, inserting if new.
    ///
    /// Returns from the in-process cache on hit; on miss, inserts into the
    /// database with `DO NOTHING` and then SELECTs the ID.
    async fn resolve_attribute_name_id(&self, name: &str) -> Result<i16, AndromedaError> {
        if let Some(id) = self.attr_name_cache.get(name) {
            return Ok(*id);
        }

        // Insert only genuinely new names (no exclusive lock on existing rows).
        sqlx::query("INSERT INTO attribute_names (name) VALUES ($1) ON CONFLICT (name) DO NOTHING")
            .bind(name)
            .execute(&self.pool)
            .await?;

        let id: i16 = sqlx::query_scalar("SELECT id FROM attribute_names WHERE name = $1")
            .bind(name)
            .fetch_one(&self.pool)
            .await?;

        self.attr_name_cache.insert(name.to_owned(), id);
        Ok(id)
    }

    /// Bulk-resolve attribute names to dictionary IDs, inserting new names.
    ///
    /// Checks the in-process cache first; only names not yet cached are sent
    /// to the database. Uses `INSERT ... DO NOTHING` to avoid exclusive row
    /// locks on existing names, then SELECTs all IDs in one round-trip.
    async fn resolve_attribute_name_ids(&self, names: &[&str]) -> Result<HashMap<String, i16>, AndromedaError> {
        if names.is_empty() {
            return Ok(HashMap::new());
        }

        let mut result = HashMap::with_capacity(names.len());
        let mut uncached: Vec<&str> = Vec::new();

        for name in names {
            if let Some(id) = self.attr_name_cache.get(*name) {
                result.insert((*name).to_owned(), *id);
            } else {
                uncached.push(name);
            }
        }

        if uncached.is_empty() {
            return Ok(result);
        }

        // Insert only genuinely new names (shared-lock-friendly).
        sqlx::query(
            "INSERT INTO attribute_names (name) \
             SELECT name FROM UNNEST($1::text[]) AS t(name) \
             ON CONFLICT (name) DO NOTHING",
        )
        .bind(&uncached)
        .execute(&self.pool)
        .await?;

        // Fetch IDs for all uncached names in one query.
        let rows = sqlx::query("SELECT id, name FROM attribute_names WHERE name = ANY($1)")
            .bind(&uncached)
            .fetch_all(&self.pool)
            .await?;

        for row in rows {
            let name: String = row.get("name");
            let id: i16 = row.get("id");
            self.attr_name_cache.insert(name.clone(), id);
            result.insert(name, id);
        }

        Ok(result)
    }

    // -------------------------------------------------------------------------
    // TEA (Temporal Entity Attributes) Operations
    // -------------------------------------------------------------------------

    /// Find or create a catalog entry, returns UUID.
    pub async fn upsert_tea(
        &self,
        entity_id: &str,
        attribute_name: &str,
        attribute_type: AttributeType,
        dataset_id: Option<&Urn>,
        value_type: ValueType,
    ) -> Result<uuid::Uuid, AndromedaError> {
        let attr_name_id = self.resolve_attribute_name_id(attribute_name).await?;

        let id: uuid::Uuid = sqlx::query_scalar(
            r#"
            INSERT INTO temporal_entity_attributes (entity_id, attribute_name_id, attribute_type, dataset_id, value_type)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (entity_id, attribute_name_id, dataset_id) DO UPDATE
            SET attribute_type = EXCLUDED.attribute_type, value_type = EXCLUDED.value_type
            RETURNING id
            "#,
        )
        .bind(entity_id)
        .bind(attr_name_id)
        .bind(attribute_type.short_name())
        .bind(dataset_id.map(ToString::to_string))
        .bind(value_type.to_string())
        .fetch_one(&self.pool)
        .await?;

        Ok(id)
    }

    /// TEA entries for a specific attribute.
    pub async fn get_teas_for_entity_attr(&self, entity_id: &str, attribute_name: &str) -> Result<Vec<TeaEntry>, AndromedaError> {
        let rows = sqlx::query(
            r#"
            SELECT tea.id, tea.entity_id, an.name AS attribute_name,
                   tea.attribute_type, tea.dataset_id, tea.value_type
            FROM temporal_entity_attributes tea
            JOIN attribute_names an ON an.id = tea.attribute_name_id
            WHERE tea.entity_id = $1 AND an.name = $2
            "#,
        )
        .bind(entity_id)
        .bind(attribute_name)
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(parse_tea_entry).collect()
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
                    DELETE FROM temporal_entity_attributes tea
                    USING attribute_names an
                    WHERE an.id = tea.attribute_name_id
                    AND tea.entity_id = $1 AND an.name = $2
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
                        DELETE FROM temporal_entity_attributes tea
                        USING attribute_names an
                        WHERE an.id = tea.attribute_name_id
                        AND tea.entity_id = $1 AND an.name = $2
                        AND tea.dataset_id = $3
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
                        DELETE FROM temporal_entity_attributes tea
                        USING attribute_names an
                        WHERE an.id = tea.attribute_name_id
                        AND tea.entity_id = $1 AND an.name = $2
                        AND tea.dataset_id IS NULL
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
    ///   1. Same (tea_id, instance_id) -- entity update re-records the same attribute instance
    ///   2. Same (tea_id, observed_at) -- temporal create_or_update merges at the same timestamp
    ///
    /// Note: INSERT ON CONFLICT is not viable here because TimescaleDB requires
    /// the partitioning column (`time`) in all unique constraints, but dedup keys
    /// do not include `time` (instance_id may appear at a new timestamp, observed_at
    /// may differ from `time`).
    pub async fn insert_history(&self, params: InsertHistoryParams<'_>) -> Result<(), AndromedaError> {
        let mut tx = self.pool.begin().await?;

        // Dedup axis 1: same instance_id (uses idx_attribute_history_dedup).
        sqlx::query("DELETE FROM attribute_history WHERE tea_id = $1 AND instance_id = $2")
            .bind(params.tea_id)
            .bind(params.instance_id)
            .execute(&mut *tx)
            .await?;

        // Dedup axis 2: same observed_at (uses idx_attribute_history_observed_at).
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
                ($1, $2, $3, $4, $5, ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON($6), 4326)), $7, $8, $9, $10, $11, $12)
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
                SELECT tea.id FROM temporal_entity_attributes tea
                JOIN attribute_names an ON an.id = tea.attribute_name_id
                WHERE tea.entity_id = $2 AND an.name = $3
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
            return Err(AndromedaError::temporal_not_found(format!("Attribute instance {} not found", instance_id)));
        }

        Ok(())
    }

    /// Modify a specific attribute instance.
    pub async fn update_instance(&self, params: UpdateInstanceParams<'_>) -> Result<(), AndromedaError> {
        let result = sqlx::query(
            r#"
            UPDATE attribute_history
            SET value_number = $4, value_string = $5, value_bool = $6,
                value_geo = ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON($7), 4326)),
                value_object = $8, value_datetime = $9, value_uri = $10,
                observed_at = $11, metadata = $12
            WHERE instance_id = $1
            AND tea_id IN (
                SELECT tea.id FROM temporal_entity_attributes tea
                JOIN attribute_names an ON an.id = tea.attribute_name_id
                WHERE tea.entity_id = $2 AND an.name = $3
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
            return Err(AndromedaError::temporal_not_found(format!(
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
            SELECT ah.*, an.name AS attribute_name, tea.attribute_type, tea.value_type
            FROM attribute_history ah
            JOIN temporal_entity_attributes tea ON tea.id = ah.tea_id
            JOIN attribute_names an ON an.id = tea.attribute_name_id
            WHERE ah.instance_id = $1
            AND tea.entity_id = $2 AND an.name = $3
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

fn parse_tea_entry(row: &PgRow) -> Result<TeaEntry, AndromedaError> {
    let attribute_name_raw: String = row.try_get("attribute_name")?;
    let attribute_name = IriBuf::new(attribute_name_raw.clone())
        .map_err(|e| AndromedaError::internal(format!("Invalid temporal_entity_attributes.attribute_name '{}': {}", attribute_name_raw, e)))?;

    let attribute_type_raw: String = row.try_get("attribute_type")?;
    let attribute_type = AttributeType::from_str(&attribute_type_raw)
        .map_err(|e| AndromedaError::internal(format!("Invalid temporal_entity_attributes.attribute_type '{}': {}", attribute_type_raw, e)))?;

    let dataset_id = row.try_get::<Option<String>, _>("dataset_id")?.map(|raw| Urn::from_str(&raw)).transpose()?;

    let value_type_raw: String = row.try_get("value_type")?;
    let value_type = ValueType::from_str(&value_type_raw)
        .map_err(|e| AndromedaError::internal(format!("Invalid temporal_entity_attributes.value_type '{}': {}", value_type_raw, e)))?;

    Ok(TeaEntry {
        attribute_name,
        attribute_type,
        dataset_id,
        value_type,
    })
}

// ---------------------------------------------------------------------------
// Bulk batch types and operations
// ---------------------------------------------------------------------------

/// Input for a single TEA catalog entry in a bulk upsert.
#[derive(Debug)]
pub struct TeaBatchEntry {
    /// Entity identifier.
    pub entity_id: String,
    /// Expanded IRI of the attribute.
    pub attribute_name: String,
    /// NGSI-LD attribute meta-type.
    pub attribute_type: AttributeType,
    /// Multi-attribute discriminator; `None` for the default instance.
    pub dataset_id: Option<String>,
    /// Runtime value type discriminator.
    pub value_type: ValueType,
}

/// Result of a bulk TEA upsert: maps back to the input entry via its natural key.
#[derive(Debug)]
pub struct TeaBatchResult {
    /// Surrogate UUID assigned by the database.
    pub id: uuid::Uuid,
    /// Entity identifier (from the input).
    pub entity_id: String,
    /// Attribute IRI (from the input).
    pub attribute_name: String,
    /// Dataset ID (from the input).
    pub dataset_id: Option<String>,
}

/// Input for a single attribute history observation in a bulk insert.
#[derive(Debug)]
pub struct HistoryBatchEntry {
    /// TimescaleDB partition key.
    pub time: DateTime<Utc>,
    /// Foreign key into the TEA catalog.
    pub tea_id: uuid::Uuid,
    /// Numeric value column.
    pub value_number: Option<f64>,
    /// String value column.
    pub value_string: Option<String>,
    /// Boolean value column.
    pub value_bool: Option<bool>,
    /// GeoJSON geometry string.
    pub value_geo: Option<String>,
    /// JSON object value column.
    pub value_object: Option<Value>,
    /// DateTime value column.
    pub value_datetime: Option<DateTime<Utc>>,
    /// URI value column (for Relationships).
    pub value_uri: Option<String>,
    /// Unique instance identifier.
    pub instance_id: String,
    /// Application-level observation timestamp.
    pub observed_at: Option<DateTime<Utc>>,
    /// Additional metadata (sub-attributes, unitCode, etc.).
    pub metadata: Option<Value>,
}

impl TemporalRepository {
    /// Bulk upsert TEA catalog entries in a single SQL round-trip using `UNNEST`.
    ///
    /// Returns one [`TeaBatchResult`] per input entry with the resolved UUID,
    /// enabling the caller to build history rows without additional queries.
    pub async fn upsert_tea_batch(&self, entries: &[TeaBatchEntry]) -> Result<Vec<TeaBatchResult>, AndromedaError> {
        if entries.is_empty() {
            return Ok(Vec::new());
        }

        // Step 1: resolve all distinct attribute names to dictionary IDs.
        let distinct_names: Vec<&str> = {
            let mut seen = std::collections::HashSet::new();
            entries
                .iter()
                .filter_map(|e| seen.insert(e.attribute_name.as_str()).then_some(e.attribute_name.as_str()))
                .collect()
        };
        let name_to_id = self.resolve_attribute_name_ids(&distinct_names).await?;

        // Step 2: build column vectors with attribute_name_id instead of attribute_name.
        let mut entity_ids = Vec::with_capacity(entries.len());
        let mut attr_name_ids: Vec<i16> = Vec::with_capacity(entries.len());
        let mut attribute_types = Vec::with_capacity(entries.len());
        let mut dataset_ids = Vec::with_capacity(entries.len());
        let mut value_types = Vec::with_capacity(entries.len());

        for e in entries {
            let attr_id = name_to_id
                .get(e.attribute_name.as_str())
                .ok_or_else(|| AndromedaError::internal(format!("Attribute name '{}' not found in dictionary after resolve", e.attribute_name)))?;
            entity_ids.push(e.entity_id.as_str());
            attr_name_ids.push(*attr_id);
            attribute_types.push(e.attribute_type.short_name());
            dataset_ids.push(e.dataset_id.as_deref());
            value_types.push(e.value_type.as_str());
        }

        let rows = sqlx::query(
            r#"
            INSERT INTO temporal_entity_attributes (entity_id, attribute_name_id, attribute_type, dataset_id, value_type)
            SELECT d.entity_id, d.attribute_name_id, d.attribute_type, d.dataset_id, d.value_type
            FROM UNNEST($1::text[], $2::smallint[], $3::text[], $4::text[], $5::text[])
                AS d(entity_id, attribute_name_id, attribute_type, dataset_id, value_type)
            ON CONFLICT (entity_id, attribute_name_id, dataset_id) DO UPDATE
            SET attribute_type = EXCLUDED.attribute_type, value_type = EXCLUDED.value_type
            RETURNING id, entity_id,
                (SELECT an.name FROM attribute_names an WHERE an.id = temporal_entity_attributes.attribute_name_id) AS attribute_name,
                dataset_id
            "#,
        )
        .bind(&entity_ids)
        .bind(&attr_name_ids)
        .bind(&attribute_types)
        .bind(&dataset_ids)
        .bind(&value_types)
        .fetch_all(&self.pool)
        .await?;

        let results = rows
            .into_iter()
            .map(|row| TeaBatchResult {
                id: row.get("id"),
                entity_id: row.get("entity_id"),
                attribute_name: row.get("attribute_name"),
                dataset_id: row.get("dataset_id"),
            })
            .collect();
        Ok(results)
    }

    /// Bulk insert attribute history rows in a single transaction.
    ///
    /// Performs deduplication (DELETE conflicting instance_id / observed_at rows)
    /// then inserts all entries using `UNNEST`, all within a single transaction.
    /// This replaces N individual `BEGIN + DELETE + INSERT + COMMIT` cycles with
    /// a single `BEGIN + bulk DELETE + bulk INSERT + COMMIT`.
    pub async fn insert_history_batch(&self, entries: &[HistoryBatchEntry]) -> Result<(), AndromedaError> {
        if entries.is_empty() {
            return Ok(());
        }

        let len = entries.len();
        let mut times = Vec::with_capacity(len);
        let mut tea_ids = Vec::with_capacity(len);
        let mut value_numbers: Vec<Option<f64>> = Vec::with_capacity(len);
        let mut value_strings: Vec<Option<String>> = Vec::with_capacity(len);
        let mut value_bools: Vec<Option<bool>> = Vec::with_capacity(len);
        let mut value_geos: Vec<Option<String>> = Vec::with_capacity(len);
        let mut value_objects: Vec<Option<Value>> = Vec::with_capacity(len);
        let mut value_datetimes: Vec<Option<DateTime<Utc>>> = Vec::with_capacity(len);
        let mut value_uris: Vec<Option<String>> = Vec::with_capacity(len);
        let mut instance_ids = Vec::with_capacity(len);
        let mut observed_ats: Vec<Option<DateTime<Utc>>> = Vec::with_capacity(len);
        let mut metadatas: Vec<Option<Value>> = Vec::with_capacity(len);

        // Dedup keys for the pre-delete step
        let mut dedup_tea_ids = Vec::with_capacity(len);
        let mut dedup_instance_ids = Vec::with_capacity(len);
        let mut dedup_observed_ats: Vec<Option<DateTime<Utc>>> = Vec::with_capacity(len);

        for e in entries {
            times.push(e.time);
            tea_ids.push(e.tea_id);
            value_numbers.push(e.value_number);
            value_strings.push(e.value_string.clone()); // clone: owned Option<String> for sqlx bind vector
            value_bools.push(e.value_bool);
            value_geos.push(e.value_geo.clone()); // clone: owned Option<String> for sqlx bind vector
            value_objects.push(e.value_object.clone()); // clone: owned Option<Value> for sqlx bind vector
            value_datetimes.push(e.value_datetime);
            value_uris.push(e.value_uri.clone()); // clone: owned Option<String> for sqlx bind vector
            instance_ids.push(e.instance_id.as_str());
            observed_ats.push(e.observed_at);
            metadatas.push(e.metadata.clone()); // clone: owned Option<Value> for sqlx bind vector

            dedup_tea_ids.push(e.tea_id);
            dedup_instance_ids.push(e.instance_id.as_str());
            dedup_observed_ats.push(e.observed_at);
        }

        let mut tx = self.pool.begin().await?;

        // Dedup axis 1: delete by (tea_id, instance_id) using idx_attribute_history_dedup.
        sqlx::query(
            r#"
            DELETE FROM attribute_history ah
            USING UNNEST($1::uuid[], $2::text[])
                AS d(tea_id, instance_id)
            WHERE ah.tea_id = d.tea_id AND ah.instance_id = d.instance_id
            "#,
        )
        .bind(&dedup_tea_ids)
        .bind(&dedup_instance_ids)
        .execute(&mut *tx)
        .await?;

        // Dedup axis 2: delete by (tea_id, observed_at) for entries that have one,
        // using idx_attribute_history_observed_at.
        let filtered: Vec<(uuid::Uuid, DateTime<Utc>)> = dedup_tea_ids
            .iter()
            .zip(dedup_observed_ats.iter())
            .filter_map(|(tid, obs)| obs.map(|o| (*tid, o)))
            .collect();

        if !filtered.is_empty() {
            let (obs_tea_ids, obs_ats): (Vec<uuid::Uuid>, Vec<DateTime<Utc>>) = filtered.into_iter().unzip();
            sqlx::query(
                r#"
                DELETE FROM attribute_history ah
                USING UNNEST($1::uuid[], $2::timestamptz[])
                    AS d(tea_id, observed_at)
                WHERE ah.tea_id = d.tea_id AND ah.observed_at = d.observed_at
                "#,
            )
            .bind(&obs_tea_ids)
            .bind(&obs_ats)
            .execute(&mut *tx)
            .await?;
        }

        // Bulk insert all history entries
        sqlx::query(
            r#"
            INSERT INTO attribute_history
                (time, tea_id, value_number, value_string, value_bool,
                 value_geo, value_object, value_datetime, value_uri,
                 instance_id, observed_at, metadata)
            SELECT
                d.time, d.tea_id, d.value_number, d.value_string, d.value_bool,
                ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON(d.value_geo), 4326)),
                d.value_object, d.value_datetime, d.value_uri,
                d.instance_id, d.observed_at, d.metadata
            FROM UNNEST(
                $1::timestamptz[], $2::uuid[], $3::double precision[], $4::text[], $5::boolean[],
                $6::text[], $7::jsonb[], $8::timestamptz[], $9::text[],
                $10::text[], $11::timestamptz[], $12::jsonb[]
            ) AS d(time, tea_id, value_number, value_string, value_bool,
                   value_geo, value_object, value_datetime, value_uri,
                   instance_id, observed_at, metadata)
            "#,
        )
        .bind(&times)
        .bind(&tea_ids)
        .bind(&value_numbers)
        .bind(&value_strings)
        .bind(&value_bools)
        .bind(&value_geos)
        .bind(&value_objects)
        .bind(&value_datetimes)
        .bind(&value_uris)
        .bind(&instance_ids)
        .bind(&observed_ats)
        .bind(&metadatas)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }
}
