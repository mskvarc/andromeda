//! SQL-only repository for the NGSI-LD Temporal API.

use crate::{
    core::{config::BulkWriteMode, error::AndromedaError},
    domain::{geometry::Wkb, temporal::TeaEntry},
    query::sql::BindValue,
    repository::{
        attribute_names::AttributeNameCache,
        temporal::{
            copy::insert_history_batch_via_copy_in_tx,
            mapper::row_to_tea,
            params::{DeleteScope, HistoryBatchEntry, ScopeHistoryEntry, TeaBatchEntry, UpdateInstanceParams},
            rows::TeaBatchResultRow,
        },
        timescale_features::TimescaleFeatures,
    },
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{Postgres, Row, Transaction, postgres::PgPool};
use std::sync::Arc;

/// PostgreSQL/TimescaleDB-backed repository for NGSI-LD temporal entity data.
#[derive(Debug)]
pub struct TemporalRepository {
    pool: PgPool,
    /// Shared `attribute_names` dictionary cache.
    attribute_names: Arc<AttributeNameCache>,
    /// TSL capability probe.
    timescale_features: TimescaleFeatures,
    /// Dispatch mode for bulk history INSERTs.
    bulk_write_mode: BulkWriteMode,
}

impl TemporalRepository {
    /// Create a new repository backed by the given connection pool.
    ///
    /// `bulk_write_mode` controls dispatch for `insert_history_batch_in_tx`:
    /// `Unnest` keeps the legacy `UNNEST(..)` driver-side path; `CopyStaging`
    /// routes through a per-transaction binary-COPY staging table.
    pub fn new(pool: &PgPool, bulk_write_mode: BulkWriteMode, attribute_names: Arc<AttributeNameCache>) -> TemporalRepository {
        TemporalRepository {
            pool: pool.clone(), // clone: Arc ref-count bump
            attribute_names,
            timescale_features: TimescaleFeatures::empty(),
            bulk_write_mode,
        }
    }

    /// Replace the repository's TimescaleDB feature set.
    pub const fn with_features(mut self, features: TimescaleFeatures) -> TemporalRepository {
        self.timescale_features = features;
        self
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
    ///
    /// Deleting the newest instance of an attribute changes what "current"
    /// means, so `attribute_current` is rebuilt for the affected TEA in the
    /// same transaction: refilled from the newest surviving history row, or
    /// dropped entirely when none survives. Without this the projection keeps
    /// serving the value that was just deleted.
    pub async fn delete_instance(&self, instance_id: &str, entity_id: &str, attribute_name: &str) -> Result<(), AndromedaError> {
        let mut tx = self.pool.begin().await?;

        let deleted = sqlx::query(
            r#"
            DELETE FROM attribute_history
            WHERE instance_id = $1
            AND tea_id IN (
                SELECT tea.id FROM temporal_entity_attributes tea
                JOIN attribute_names an ON an.id = tea.attribute_name_id
                WHERE tea.entity_id = $2 AND an.name = $3
            )
            RETURNING tea_id
            "#,
        )
        .bind(instance_id)
        .bind(entity_id)
        .bind(attribute_name)
        .fetch_optional(&mut *tx)
        .await?;

        let Some(row) = deleted else {
            return Err(AndromedaError::temporal_not_found(format!("Attribute instance {instance_id} not found")));
        };
        let tea_id: uuid::Uuid = row.try_get("tea_id")?;

        sqlx::query(DELETE_ORPHANED_ATTRIBUTE_CURRENT_SQL).bind(tea_id).execute(&mut *tx).await?;
        sqlx::query(REMATERIALIZE_ATTRIBUTE_CURRENT_SQL).bind(tea_id).execute(&mut *tx).await?;

        tx.commit().await?;
        Ok(())
    }

    /// Modify a specific attribute instance.
    pub async fn update_instance(&self, params: UpdateInstanceParams<'_>) -> Result<(), AndromedaError> {
        let result = sqlx::query(
            r#"
            UPDATE attribute_history
            SET value_number = $4, value_string = $5, value_bool = $6,
                value_geo = CASE WHEN $7 IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB($7)) ELSE NULL END,
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
        let (entity_ids, attr_name_ids, attribute_types, dataset_ids, value_types) = self.tea_batch_binds(entries).await?;
        let rows = sqlx::query(TEA_UPSERT_SQL)
            .bind(&entity_ids)
            .bind(&attr_name_ids)
            .bind(&attribute_types)
            .bind(&dataset_ids)
            .bind(&value_types)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows_to_tea_results(rows))
    }

    /// Transactional variant of [`Self::upsert_tea_batch`].
    pub async fn upsert_tea_batch_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        entries: &[TeaBatchEntry],
    ) -> Result<Vec<TeaBatchResultRow>, AndromedaError> {
        if entries.is_empty() {
            return Ok(Vec::new());
        }
        let (entity_ids, attr_name_ids, attribute_types, dataset_ids, value_types) = self.tea_batch_binds(entries).await?;
        let rows = sqlx::query(TEA_UPSERT_SQL)
            .bind(&entity_ids)
            .bind(&attr_name_ids)
            .bind(&attribute_types)
            .bind(&dataset_ids)
            .bind(&value_types)
            .fetch_all(&mut **tx)
            .await?;
        Ok(rows_to_tea_results(rows))
    }

    async fn tea_batch_binds<'a>(
        &self,
        entries: &'a [TeaBatchEntry],
    ) -> Result<(Vec<&'a str>, Vec<i16>, Vec<&'static str>, Vec<Option<&'a str>>, Vec<&'static str>), AndromedaError> {
        let distinct_names: Vec<&str> = {
            let mut seen = std::collections::HashSet::new();
            entries
                .iter()
                .filter_map(|e| seen.insert(e.attribute_name.as_str()).then_some(e.attribute_name.as_str()))
                .collect()
        };
        let name_to_id = self.attribute_names.resolve_or_create(&distinct_names).await?;

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
            attr_name_ids.push(attr_id.get());
            attribute_types.push(e.attribute_type.short_name());
            dataset_ids.push(e.dataset_id.as_deref());
            value_types.push(e.value_type.as_str());
        }
        Ok((entity_ids, attr_name_ids, attribute_types, dataset_ids, value_types))
    }

    /// Bulk insert attribute history rows in a single transaction.
    pub async fn insert_history_batch(&self, entries: &[HistoryBatchEntry]) -> Result<(), AndromedaError> {
        if entries.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool.begin().await?;
        self.insert_history_batch_in_tx(&mut tx, entries).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Transactional variant of [`Self::insert_history_batch`]. Runs the
    /// dedup `DELETE`s and the history `INSERT` against the caller's
    /// transaction without committing.
    ///
    /// The `attribute_current` projection is **not** maintained here
    /// any more: callers queue the entries on the unit-of-work and the
    /// materializer worker applies them after commit (see
    /// [`crate::worker::attribute_current_materializer`]).
    pub async fn insert_history_batch_in_tx(&self, tx: &mut Transaction<'_, Postgres>, entries: &[HistoryBatchEntry]) -> Result<(), AndromedaError> {
        if entries.is_empty() {
            return Ok(());
        }

        run_history_dedup_deletes(tx, entries).await?;

        match self.bulk_write_mode {
            BulkWriteMode::Unnest => insert_history_batch_unnest_in_tx(tx, entries).await?,
            BulkWriteMode::CopyStaging => insert_history_batch_via_copy_in_tx(tx, entries).await?,
        }

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
        let mut tx = self.pool.begin().await?;
        self.insert_scope_history_batch_in_tx(&mut tx, entries).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Transactional variant of [`Self::insert_scope_history_batch`].
    pub async fn insert_scope_history_batch_in_tx(&self, tx: &mut Transaction<'_, Postgres>, entries: &[ScopeHistoryEntry]) -> Result<(), AndromedaError> {
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
        .execute(&mut **tx)
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
        .execute(&mut **tx)
        .await?;

        Ok(())
    }
}

// The upsert skips no-op writes via `WHERE ... IS DISTINCT FROM EXCLUDED ...`
// to avoid btree/WAL amplification on the steady-state path where
// `(attribute_type, value_type)` rarely change for a given TEA tuple.
// `RETURNING` therefore omits unchanged rows, so a follow-up SELECT in the
// same CTE pulls their existing ids and the result is unioned to preserve
// the contract that every input entry yields a `TeaBatchResultRow`.
// `dataset_id` is NULL-able with `UNIQUE NULLS NOT DISTINCT` semantics
// (migration 0007), so equality joins use `IS NOT DISTINCT FROM`.
const TEA_UPSERT_SQL: &str = r#"
WITH input AS (
    SELECT d.entity_id, d.attribute_name_id, d.attribute_type, d.dataset_id, d.value_type
    FROM UNNEST($1::text[], $2::smallint[], $3::text[], $4::text[], $5::text[])
        AS d(entity_id, attribute_name_id, attribute_type, dataset_id, value_type)
),
upserted AS (
    INSERT INTO temporal_entity_attributes (entity_id, attribute_name_id, attribute_type, dataset_id, value_type)
    SELECT entity_id, attribute_name_id, attribute_type, dataset_id, value_type FROM input
    ON CONFLICT (entity_id, attribute_name_id, dataset_id) DO UPDATE
    SET attribute_type = EXCLUDED.attribute_type, value_type = EXCLUDED.value_type
    WHERE temporal_entity_attributes.attribute_type IS DISTINCT FROM EXCLUDED.attribute_type
       OR temporal_entity_attributes.value_type   IS DISTINCT FROM EXCLUDED.value_type
    RETURNING id, entity_id, attribute_name_id, dataset_id
)
SELECT u.id, u.entity_id,
       an.name AS attribute_name,
       u.dataset_id
FROM upserted u
JOIN attribute_names an ON an.id = u.attribute_name_id
UNION ALL
SELECT tea.id, tea.entity_id,
       an.name AS attribute_name,
       tea.dataset_id
FROM input i
JOIN temporal_entity_attributes tea
    ON tea.entity_id          = i.entity_id
   AND tea.attribute_name_id  = i.attribute_name_id
   AND tea.dataset_id IS NOT DISTINCT FROM i.dataset_id
JOIN attribute_names an ON an.id = tea.attribute_name_id
WHERE NOT EXISTS (
    SELECT 1 FROM upserted u
    WHERE u.entity_id          = i.entity_id
      AND u.attribute_name_id  = i.attribute_name_id
      AND u.dataset_id IS NOT DISTINCT FROM i.dataset_id
)
"#;

/// Drop the projection row for a TEA whose history is now empty.
const DELETE_ORPHANED_ATTRIBUTE_CURRENT_SQL: &str = r#"
DELETE FROM attribute_current ac
WHERE ac.tea_id = $1
  AND NOT EXISTS (SELECT 1 FROM attribute_history ah WHERE ah.tea_id = $1)
"#;

/// Rebuild the projection row for one TEA from its newest surviving history row.
///
/// The `ON CONFLICT` update is unconditional — deliberately unlike the
/// materializer's staleness-guarded upsert. Deleting the newest instance makes
/// the correct current value *older* than the row already in the projection, so
/// a `modified_at <= EXCLUDED.modified_at` guard would reject exactly the write
/// that is needed here.
///
/// The `ORDER BY COALESCE(recorded_at, time) DESC` cannot use
/// `idx_attribute_history_tea_time`, so this costs one index probe per chunk
/// rather than a single seek. Acceptable on a single-instance delete, which is
/// not an ingest path; it does grow with retention.
const REMATERIALIZE_ATTRIBUTE_CURRENT_SQL: &str = r#"
INSERT INTO attribute_current (
    tea_id, entity_id, attribute_name_id, dataset_id, value_type,
    value_number, value_string, value_bool, value_geo,
    value_object, value_datetime, value_uri, modified_at
)
SELECT
    latest.tea_id, tea.entity_id, tea.attribute_name_id, tea.dataset_id, tea.value_type,
    latest.value_number, latest.value_string, latest.value_bool, latest.value_geo,
    latest.value_object, latest.value_datetime, latest.value_uri,
    COALESCE(latest.recorded_at, latest.time)
FROM (
    SELECT * FROM attribute_history ah
    WHERE ah.tea_id = $1
    ORDER BY COALESCE(ah.recorded_at, ah.time) DESC
    LIMIT 1
) AS latest
JOIN temporal_entity_attributes tea ON tea.id = latest.tea_id
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
"#;

fn rows_to_tea_results(rows: Vec<sqlx::postgres::PgRow>) -> Vec<TeaBatchResultRow> {
    rows.into_iter()
        .map(|row| TeaBatchResultRow {
            id: row.get("id"),
            entity_id: row.get("entity_id"),
            attribute_name: row.get("attribute_name"),
            dataset_id: row.get("dataset_id"),
        })
        .collect()
}

/// Run the dedup `DELETE`s that must precede a history-batch INSERT.
///
/// 1. `(tea_id, instance_id)` pass — only when at least one entry has a
///    client-supplied instance_id (server-generated UUIDv4 ids are
///    statistically unique, so the DELETE would always be zero-yield).
/// 2. `(tea_id, observed_at)` pass — only when at least one entry carries
///    a non-null `observed_at`.
async fn run_history_dedup_deletes(tx: &mut Transaction<'_, Postgres>, entries: &[HistoryBatchEntry]) -> Result<(), AndromedaError> {
    let mut any_instance_client_supplied = false;
    let mut tea_ids = Vec::with_capacity(entries.len());
    let mut instance_ids = Vec::with_capacity(entries.len());
    let mut observed_pairs: Vec<(uuid::Uuid, DateTime<Utc>)> = Vec::with_capacity(entries.len());
    for e in entries {
        tea_ids.push(e.tea_id);
        instance_ids.push(e.instance_id.as_str());
        if let Some(obs) = e.observed_at {
            observed_pairs.push((e.tea_id, obs));
        }
        any_instance_client_supplied |= e.instance_id_client_supplied;
    }

    if any_instance_client_supplied {
        sqlx::query(
            r#"
            DELETE FROM attribute_history ah
            USING UNNEST($1::uuid[], $2::text[])
                AS d(tea_id, instance_id)
            WHERE ah.tea_id = d.tea_id AND ah.instance_id = d.instance_id
            "#,
        )
        .bind(&tea_ids)
        .bind(&instance_ids)
        .execute(&mut **tx)
        .await?;
    }

    if !observed_pairs.is_empty() {
        let (obs_tea_ids, obs_ats): (Vec<uuid::Uuid>, Vec<DateTime<Utc>>) = observed_pairs.into_iter().unzip();
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
        .execute(&mut **tx)
        .await?;
    }

    Ok(())
}

/// Driver-side `UNNEST(..)` history-batch INSERT.
///
/// Kept for the `BulkWriteMode::Unnest` branch and as a fall-back if the
/// binary-COPY path is disabled at runtime.
async fn insert_history_batch_unnest_in_tx(tx: &mut Transaction<'_, Postgres>, entries: &[HistoryBatchEntry]) -> Result<(), AndromedaError> {
    let len = entries.len();
    let mut times = Vec::with_capacity(len);
    let mut tea_ids = Vec::with_capacity(len);
    let mut value_numbers: Vec<Option<f64>> = Vec::with_capacity(len);
    let mut value_strings: Vec<Option<&str>> = Vec::with_capacity(len);
    let mut value_bools: Vec<Option<bool>> = Vec::with_capacity(len);
    let mut value_geos: Vec<Option<&Wkb>> = Vec::with_capacity(len);
    let mut value_objects: Vec<Option<&Value>> = Vec::with_capacity(len);
    let mut value_datetimes: Vec<Option<DateTime<Utc>>> = Vec::with_capacity(len);
    let mut value_uris: Vec<Option<&str>> = Vec::with_capacity(len);
    let mut instance_ids = Vec::with_capacity(len);
    let mut observed_ats: Vec<Option<DateTime<Utc>>> = Vec::with_capacity(len);
    let mut metadatas: Vec<Option<&Value>> = Vec::with_capacity(len);
    let mut recorded_ats: Vec<DateTime<Utc>> = Vec::with_capacity(len);

    for e in entries {
        times.push(e.time);
        tea_ids.push(e.tea_id);
        value_numbers.push(e.value_number);
        value_strings.push(e.value_string.as_deref());
        value_bools.push(e.value_bool);
        value_geos.push(e.value_geo.as_ref());
        value_objects.push(e.value_object.as_ref());
        value_datetimes.push(e.value_datetime);
        value_uris.push(e.value_uri.as_deref());
        instance_ids.push(e.instance_id.as_str());
        observed_ats.push(e.observed_at);
        metadatas.push(e.metadata.as_ref());
        recorded_ats.push(e.recorded_at);
    }

    sqlx::query(
        r#"
        INSERT INTO attribute_history
            (time, tea_id, value_number, value_string, value_bool,
             value_geo, value_object, value_datetime, value_uri,
             instance_id, observed_at, metadata, recorded_at)
        SELECT
            d.time, d.tea_id, d.value_number, d.value_string, d.value_bool,
            CASE WHEN d.value_geo IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB(d.value_geo)) ELSE NULL END,
            d.value_object, d.value_datetime, d.value_uri,
            d.instance_id, d.observed_at, d.metadata, d.recorded_at
        FROM UNNEST(
            $1::timestamptz[], $2::uuid[], $3::double precision[], $4::text[], $5::boolean[],
            $6::bytea[], $7::jsonb[], $8::timestamptz[], $9::text[],
            $10::text[], $11::timestamptz[], $12::jsonb[], $13::timestamptz[]
        ) AS d(time, tea_id, value_number, value_string, value_bool,
               value_geo, value_object, value_datetime, value_uri,
               instance_id, observed_at, metadata, recorded_at)
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
    .bind(&recorded_ats)
    .execute(&mut **tx)
    .await?;

    Ok(())
}
