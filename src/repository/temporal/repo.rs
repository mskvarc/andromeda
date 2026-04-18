//! SQL-only repository for the NGSI-LD Temporal API.

use crate::{
    core::error::AndromedaError,
    domain::temporal::TeaEntry,
    query::sql::BindValue,
    repository::{
        temporal::{
            mapper::row_to_tea,
            params::{DeleteScope, HistoryBatchEntry, ScopeHistoryEntry, TeaBatchEntry, UpdateInstanceParams},
            rows::TeaBatchResultRow,
        },
        timescale_features::TimescaleFeatures,
    },
};
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde_json::Value;
use sqlx::{Postgres, Row, Transaction, postgres::PgPool};
use std::{collections::HashMap, sync::Arc};

/// PostgreSQL/TimescaleDB-backed repository for NGSI-LD temporal entity data.
#[derive(Debug)]
pub struct TemporalRepository {
    pool: PgPool,
    /// In-process cache for attribute name dictionary lookups.
    attr_name_cache: Arc<DashMap<String, i16>>,
    /// TSL capability probe.
    timescale_features: TimescaleFeatures,
}

impl TemporalRepository {
    /// Create a new repository backed by the given connection pool.
    pub fn new(pool: &PgPool) -> TemporalRepository {
        TemporalRepository {
            pool: pool.clone(), // clone: Arc ref-count bump
            attr_name_cache: Arc::new(DashMap::new()),
            timescale_features: TimescaleFeatures::empty(),
        }
    }

    /// Replace the repository's TimescaleDB feature set.
    pub const fn with_features(mut self, features: TimescaleFeatures) -> TemporalRepository {
        self.timescale_features = features;
        self
    }

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

        sqlx::query(
            "INSERT INTO attribute_names (name) \
             SELECT name FROM UNNEST($1::text[]) AS t(name) \
             ON CONFLICT (name) DO NOTHING",
        )
        .bind(&uncached)
        .execute(&self.pool)
        .await?;

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

        rows.iter().map(row_to_tea).collect()
    }

    /// Distinct attribute names recorded for the given temporal entity.
    pub async fn get_attribute_names_for_entity(&self, entity_id: &str) -> Result<Vec<String>, AndromedaError> {
        let rows = sqlx::query(
            r#"
            SELECT DISTINCT an.name AS attribute_name
            FROM temporal_entity_attributes tea
            JOIN attribute_names an ON an.id = tea.attribute_name_id
            WHERE tea.entity_id = $1
            "#,
        )
        .bind(entity_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.iter().filter_map(|row| row.try_get::<String, _>("attribute_name").ok()).collect())
    }

    /// Check if any temporal data exists for an entity.
    pub async fn tea_exists_for_entity(&self, entity_id: &str) -> Result<bool, AndromedaError> {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM temporal_entity_attributes WHERE entity_id = $1)")
            .bind(entity_id)
            .fetch_one(&self.pool)
            .await?;

        Ok(exists)
    }

    /// Delete all TEAs for an entity (cascades to history). Returns rows affected.
    pub async fn delete_teas_for_entity(&self, entity_id: &str) -> Result<u64, AndromedaError> {
        let result = sqlx::query("DELETE FROM temporal_entity_attributes WHERE entity_id = $1")
            .bind(entity_id)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected())
    }

    /// Delete specific TEA entry.
    pub async fn delete_tea(&self, entity_id: &str, attribute_name: &str, dataset_id: Option<&str>, scope: DeleteScope) -> Result<u64, AndromedaError> {
        let result = match scope {
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
                .await?
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
                    .await?
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
                    .await?
                }
            }
        };

        Ok(result.rows_affected())
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
            return Err(AndromedaError::temporal_not_found(format!("Attribute instance {instance_id} not found")));
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

    /// Bulk upsert TEA catalog entries in a single SQL round-trip using `UNNEST`.
    pub async fn upsert_tea_batch(&self, entries: &[TeaBatchEntry]) -> Result<Vec<TeaBatchResultRow>, AndromedaError> {
        if entries.is_empty() {
            return Ok(Vec::new());
        }

        let distinct_names: Vec<&str> = {
            let mut seen = std::collections::HashSet::new();
            entries
                .iter()
                .filter_map(|e| seen.insert(e.attribute_name.as_str()).then_some(e.attribute_name.as_str()))
                .collect()
        };
        let name_to_id = self.resolve_attribute_name_ids(&distinct_names).await?;

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
            .map(|row| TeaBatchResultRow {
                id: row.get("id"),
                entity_id: row.get("entity_id"),
                attribute_name: row.get("attribute_name"),
                dataset_id: row.get("dataset_id"),
            })
            .collect();
        Ok(results)
    }

    /// Bulk insert attribute history rows in a single transaction.
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

        let mut dedup_tea_ids = Vec::with_capacity(len);
        let mut dedup_instance_ids = Vec::with_capacity(len);
        let mut dedup_observed_ats: Vec<Option<DateTime<Utc>>> = Vec::with_capacity(len);

        for e in entries {
            times.push(e.time);
            tea_ids.push(e.tea_id);
            value_numbers.push(e.value_number);
            value_strings.push(e.value_string.clone());
            value_bools.push(e.value_bool);
            value_geos.push(e.value_geo.clone());
            value_objects.push(e.value_object.clone());
            value_datetimes.push(e.value_datetime);
            value_uris.push(e.value_uri.clone());
            instance_ids.push(e.instance_id.as_str());
            observed_ats.push(e.observed_at);
            metadatas.push(e.metadata.clone());

            dedup_tea_ids.push(e.tea_id);
            dedup_instance_ids.push(e.instance_id.as_str());
            dedup_observed_ats.push(e.observed_at);
        }

        let mut tx = self.pool.begin().await?;

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

        sync_attribute_current_batch(&mut tx, entries).await?;

        tx.commit().await?;
        Ok(())
    }

    /// Insert a single scope history observation.
    pub async fn insert_scope_history(&self, entry: &ScopeHistoryEntry) -> Result<(), AndromedaError> {
        sqlx::query(
            r#"
            INSERT INTO scope_history (time, entity_id, scope_path, instance_id)
            VALUES ($1, $2, $3::ltree, $4)
            ON CONFLICT (entity_id, time, instance_id) DO NOTHING
            "#,
        )
        .bind(entry.time)
        .bind(&entry.entity_id)
        .bind(&entry.scope_path)
        .bind(&entry.instance_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Bulk insert scope history observations in a single transaction.
    pub async fn insert_scope_history_batch(&self, entries: &[ScopeHistoryEntry]) -> Result<(), AndromedaError> {
        if entries.is_empty() {
            return Ok(());
        }

        let len = entries.len();
        let mut times = Vec::with_capacity(len);
        let mut entity_ids = Vec::with_capacity(len);
        let mut scope_paths = Vec::with_capacity(len);
        let mut instance_ids = Vec::with_capacity(len);

        for e in entries {
            times.push(e.time);
            entity_ids.push(e.entity_id.as_str());
            scope_paths.push(e.scope_path.as_str());
            instance_ids.push(e.instance_id.as_str());
        }

        let mut tx = self.pool.begin().await?;

        sqlx::query(
            r#"
            DELETE FROM scope_history sh
            USING UNNEST($1::text[], $2::text[])
                AS d(entity_id, instance_id)
            WHERE sh.entity_id = d.entity_id AND sh.instance_id = d.instance_id
            "#,
        )
        .bind(&entity_ids)
        .bind(&instance_ids)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO scope_history (time, entity_id, scope_path, instance_id)
            SELECT d.time, d.entity_id, d.scope_path::ltree, d.instance_id
            FROM UNNEST($1::timestamptz[], $2::text[], $3::text[], $4::text[])
                AS d(time, entity_id, scope_path, instance_id)
            "#,
        )
        .bind(&times)
        .bind(&entity_ids)
        .bind(&scope_paths)
        .bind(&instance_ids)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }
}

async fn sync_attribute_current_batch(tx: &mut Transaction<'_, Postgres>, entries: &[HistoryBatchEntry]) -> Result<(), AndromedaError> {
    if entries.is_empty() {
        return Ok(());
    }

    let len = entries.len();
    let mut tea_ids = Vec::with_capacity(len);
    let mut times = Vec::with_capacity(len);
    let mut value_numbers: Vec<Option<f64>> = Vec::with_capacity(len);
    let mut value_strings: Vec<Option<&str>> = Vec::with_capacity(len);
    let mut value_bools: Vec<Option<bool>> = Vec::with_capacity(len);
    let mut value_geos: Vec<Option<&str>> = Vec::with_capacity(len);
    let mut value_objects: Vec<Option<&Value>> = Vec::with_capacity(len);
    let mut value_datetimes: Vec<Option<DateTime<Utc>>> = Vec::with_capacity(len);
    let mut value_uris: Vec<Option<&str>> = Vec::with_capacity(len);

    for e in entries {
        tea_ids.push(e.tea_id);
        times.push(e.time);
        value_numbers.push(e.value_number);
        value_strings.push(e.value_string.as_deref());
        value_bools.push(e.value_bool);
        value_geos.push(e.value_geo.as_deref());
        value_objects.push(e.value_object.as_ref());
        value_datetimes.push(e.value_datetime);
        value_uris.push(e.value_uri.as_deref());
    }

    sqlx::query(
        r#"
        INSERT INTO attribute_current (
            tea_id, entity_id, attribute_name_id, dataset_id, value_type,
            value_number, value_string, value_bool, value_geo,
            value_object, value_datetime, value_uri, modified_at
        )
        SELECT DISTINCT ON (d.tea_id)
            d.tea_id, tea.entity_id, tea.attribute_name_id, tea.dataset_id, tea.value_type,
            d.value_number, d.value_string, d.value_bool,
            ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON(d.value_geo), 4326)),
            d.value_object, d.value_datetime, d.value_uri, d.time
        FROM UNNEST(
            $1::uuid[], $2::timestamptz[], $3::double precision[], $4::text[], $5::boolean[],
            $6::text[], $7::jsonb[], $8::timestamptz[], $9::text[]
        ) AS d(tea_id, time, value_number, value_string, value_bool,
               value_geo, value_object, value_datetime, value_uri)
        JOIN temporal_entity_attributes tea ON tea.id = d.tea_id
        ORDER BY d.tea_id, d.time DESC
        ON CONFLICT (tea_id) DO UPDATE SET
            entity_id         = EXCLUDED.entity_id,
            attribute_name_id = EXCLUDED.attribute_name_id,
            dataset_id        = EXCLUDED.dataset_id,
            value_type        = EXCLUDED.value_type,
            value_number      = EXCLUDED.value_number,
            value_string      = EXCLUDED.value_string,
            value_bool        = EXCLUDED.value_bool,
            value_geo         = EXCLUDED.value_geo,
            value_object      = EXCLUDED.value_object,
            value_datetime    = EXCLUDED.value_datetime,
            value_uri         = EXCLUDED.value_uri,
            modified_at       = EXCLUDED.modified_at
        WHERE attribute_current.modified_at <= EXCLUDED.modified_at
        "#,
    )
    .bind(&tea_ids)
    .bind(&times)
    .bind(&value_numbers)
    .bind(&value_strings)
    .bind(&value_bools)
    .bind(&value_geos)
    .bind(&value_objects)
    .bind(&value_datetimes)
    .bind(&value_uris)
    .execute(&mut **tx)
    .await?;

    Ok(())
}
