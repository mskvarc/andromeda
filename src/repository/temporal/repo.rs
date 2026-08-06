//! SQL-only repository for the NGSI-LD Temporal API.

use crate::{
    core::{config::BulkWriteMode, error::AndromedaError},
    repository::{
        attribute_names::AttributeNameCache,
        ewkb_binding::geometry_bytes,
        temporal::{
            copy::insert_history_batch_via_copy_in_tx,
            entry::TeaEntry,
            mapper::row_to_tea,
            params::{DeleteScope, HistoryBatchEntry, ScopeHistoryEntry, TeaBatchEntry, UpdateInstanceParams},
            rows::TeaBatchResultRow,
        },
        timescale_features::TimescaleFeatures,
    },
};
use andromeda_postgresql_query::{attribute_name_id::AttributeNameId, bind::BindValue};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{
    Postgres,
    Row,
    Transaction,
    postgres::{PgConnection, PgPool},
};
use std::{
    collections::{HashMap, HashSet, hash_map::Entry},
    sync::Arc,
};

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

        repair_attribute_current(&mut tx, tea_id).await?;

        tx.commit().await?;
        Ok(())
    }

    /// Modify a specific attribute instance.
    ///
    /// Rewriting the newest instance of an attribute changes what "current"
    /// means, so `attribute_current` is rebuilt for the affected TEA in the same
    /// transaction. Without this a `PATCH` of the newest instance leaves the
    /// projection holding the superseded value, and `q` filters compiled against
    /// `attribute_current` answer from it.
    pub async fn update_instance(&self, params: UpdateInstanceParams<'_>) -> Result<(), AndromedaError> {
        let mut tx = self.pool.begin().await?;

        let updated = sqlx::query(UPDATE_INSTANCE_SQL)
            .bind(params.instance_id)
            .bind(params.entity_id)
            .bind(params.attribute_name)
            .bind(params.values.value_number)
            .bind(params.values.value_string)
            .bind(params.values.value_bool)
            .bind(geometry_bytes(params.values.value_geo)?)
            .bind(params.values.value_object)
            .bind(params.values.value_datetime)
            .bind(params.values.value_uri)
            .bind(params.observed_at)
            .bind(params.values.metadata)
            .fetch_optional(&mut *tx)
            .await?;

        let Some(row) = updated else {
            return Err(AndromedaError::temporal_not_found(format!(
                "Attribute instance {} not found",
                params.instance_id
            )));
        };
        let tea_id: uuid::Uuid = row.try_get("tea_id")?;

        repair_attribute_current(&mut tx, tea_id).await?;

        tx.commit().await?;
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

    /// Bulk upsert TEA catalog entries in a single SQL round-trip using `UNNEST`,
    /// inside the caller's transaction.
    ///
    /// Entries sharing a catalogue key collapse into one statement row — see
    /// [`tea_batch_binds`] — so the result holds one row per *distinct* key
    /// rather than one per entry. Callers pair their history rows to TEA ids by
    /// that same key, so this is the shape they need.
    pub async fn upsert_tea_batch_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        entries: &[TeaBatchEntry],
    ) -> Result<Vec<TeaBatchResultRow>, AndromedaError> {
        if entries.is_empty() {
            return Ok(Vec::new());
        }
        let name_to_id = self.attribute_names.resolve_or_create(&distinct_attribute_names(entries)).await?;
        let binds = tea_batch_binds(entries, &name_to_id)?;
        let rows = sqlx::query(TEA_UPSERT_SQL)
            .bind(&binds.entity_ids)
            .bind(&binds.attribute_name_ids)
            .bind(&binds.attribute_types)
            .bind(&binds.dataset_ids)
            .bind(&binds.value_types)
            .fetch_all(&mut **tx)
            .await?;
        Ok(rows_to_tea_results(rows))
    }

    /// Bulk insert attribute history rows inside the caller's transaction.
    ///
    /// Runs the dedup `DELETE`s and the history `INSERT` against the caller's
    /// transaction without committing.
    ///
    /// The `attribute_current` projection is **not** maintained here
    /// any more: callers queue the entries on commit and the materializer
    /// worker applies them afterwards (see
    /// [`crate::repository::attribute_current`]).
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

    /// Bulk insert scope history observations inside the caller's transaction.
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

/// Column-major bind arrays for [`TEA_UPSERT_SQL`]: one element per statement
/// row, in the order `UNNEST` unpacks them.
#[derive(Default)]
struct TeaBatchBinds<'a> {
    entity_ids: Vec<&'a str>,
    attribute_name_ids: Vec<i16>,
    attribute_types: Vec<&'static str>,
    dataset_ids: Vec<Option<&'a str>>,
    value_types: Vec<&'static str>,
}

/// The distinct attribute IRIs across `entries`, in first-seen order.
fn distinct_attribute_names(entries: &[TeaBatchEntry]) -> Vec<&str> {
    let mut seen = HashSet::with_capacity(entries.len());
    entries
        .iter()
        .filter_map(|e| seen.insert(e.attribute_name.as_str()).then_some(e.attribute_name.as_str()))
        .collect()
}

/// Build the bind arrays for [`TEA_UPSERT_SQL`], collapsing entries that share
/// the catalogue's conflict key `(entity_id, attribute_name_id, dataset_id)`.
///
/// The collapse is not an optimisation, it is what makes the statement legal.
/// A temporal write carries one [`TeaBatchEntry`] per attribute *instance*, so a
/// request holding several instances of one attribute — which 5.6.12.4 requires
/// to be stored, all of them — proposes the same catalogue key several times.
/// PostgreSQL rejects that outright: an `INSERT ... ON CONFLICT DO UPDATE` whose
/// input contains a key twice raises `21000`, "cannot affect row a second time",
/// and takes the statement, the transaction and the whole request with it. The
/// history rows are unaffected by the collapse — they are keyed by `instance_id`
/// and every one of them is still written, pointing at the single TEA row.
///
/// The last entry for a key describes the resulting row. `attribute_type` and
/// `value_type` record what the attribute currently *is*, and the pre-batching
/// write path applied instances one statement at a time, so the newest
/// description is the one that survived there too.
fn tea_batch_binds<'a>(entries: &'a [TeaBatchEntry], name_to_id: &HashMap<String, AttributeNameId>) -> Result<TeaBatchBinds<'a>, AndromedaError> {
    let mut binds = TeaBatchBinds {
        entity_ids: Vec::with_capacity(entries.len()),
        attribute_name_ids: Vec::with_capacity(entries.len()),
        attribute_types: Vec::with_capacity(entries.len()),
        dataset_ids: Vec::with_capacity(entries.len()),
        value_types: Vec::with_capacity(entries.len()),
    };
    let mut row_of: HashMap<(&'a str, i16, Option<&'a str>), usize> = HashMap::with_capacity(entries.len());

    for e in entries {
        let attribute_name_id = name_to_id
            .get(e.attribute_name.as_str())
            .ok_or_else(|| AndromedaError::internal(format!("Attribute name '{}' not found in dictionary after resolve", e.attribute_name)))?
            .get();
        match row_of.entry((e.entity_id.as_str(), attribute_name_id, e.dataset_id.as_deref())) {
            Entry::Occupied(occupied) => {
                let row = *occupied.get();
                binds.attribute_types[row] = e.attribute_type.short_name();
                binds.value_types[row] = e.value_type.as_str();
            }
            Entry::Vacant(vacant) => {
                vacant.insert(binds.entity_ids.len());
                binds.entity_ids.push(e.entity_id.as_str());
                binds.attribute_name_ids.push(attribute_name_id);
                binds.attribute_types.push(e.attribute_type.short_name());
                binds.dataset_ids.push(e.dataset_id.as_deref());
                binds.value_types.push(e.value_type.as_str());
            }
        }
    }

    Ok(binds)
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

/// Rewrite one attribute instance in place.
///
/// `RETURNING tea_id` rather than the instance id: the caller needs the owning
/// TEA to rebuild `attribute_current`, and the instance id it already has.
const UPDATE_INSTANCE_SQL: &str = r#"
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
RETURNING tea_id
"#;

/// Rebuild one TEA's `attribute_current` row from `attribute_history`.
///
/// `attribute_history` is the canonical record, so this recomputes the
/// projection rather than trusting any in-memory state. It is used from two
/// places: after deleting an attribute instance, where the surviving newest row
/// becomes current; and by the materializer when an apply fails, where it turns
/// a row the projection could not accept into a correct outcome instead of a
/// silent gap.
///
/// Both cases are covered without special-casing. If the owning TEA is gone the
/// refill selects nothing and the orphan delete removes any stale row, which is
/// the right answer. If the TEA is present the newest surviving observation is
/// restored.
pub async fn repair_attribute_current(conn: &mut PgConnection, tea_id: uuid::Uuid) -> Result<(), AndromedaError> {
    sqlx::query(DELETE_ORPHANED_ATTRIBUTE_CURRENT_SQL).bind(tea_id).execute(&mut *conn).await?;
    sqlx::query(REMATERIALIZE_ATTRIBUTE_CURRENT_SQL).bind(tea_id).execute(&mut *conn).await?;
    Ok(())
}

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
    let mut value_geos: Vec<Option<Vec<u8>>> = Vec::with_capacity(len);
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
        value_geos.push(geometry_bytes(e.value_geo.as_ref())?);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::temporal::value_type::ValueType;
    use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;

    const ENTITY: &str = "urn:ngsi-ld:Vehicle:A100";
    const SPEED: &str = "https://uri.etsi.org/ngsi-ld/default-context/speed";
    const BRAND: &str = "https://uri.etsi.org/ngsi-ld/default-context/brand";

    fn tea_entry(attribute_name: &str, dataset_id: Option<&str>, value_type: ValueType) -> TeaBatchEntry {
        TeaBatchEntry {
            entity_id: ENTITY.to_owned(),
            attribute_name: attribute_name.to_owned(),
            attribute_type: AttributeType::Property,
            dataset_id: dataset_id.map(ToOwned::to_owned),
            value_type,
        }
    }

    /// Dictionary stand-in: the ids are arbitrary, only their distinctness matters.
    fn name_ids() -> HashMap<String, AttributeNameId> {
        HashMap::from([(SPEED.to_owned(), AttributeNameId::from(1i16)), (BRAND.to_owned(), AttributeNameId::from(2i16))])
    }

    #[test]
    fn update_instance_returns_the_owning_tea_id() {
        // The projection repair that follows the UPDATE keys on the TEA. The
        // statement used to return instance_id, which the caller already holds,
        // leaving attribute_current serving the value the PATCH superseded.
        assert!(UPDATE_INSTANCE_SQL.contains("RETURNING tea_id"));
        assert!(!UPDATE_INSTANCE_SQL.contains("RETURNING instance_id"));
    }

    #[test]
    fn the_dedupe_key_is_the_statements_conflict_target() {
        // tea_batch_binds collapses on (entity_id, attribute_name_id,
        // dataset_id). If the statement's conflict target ever moves, the
        // collapse stops matching it and 21000 comes back.
        assert!(TEA_UPSERT_SQL.contains("ON CONFLICT (entity_id, attribute_name_id, dataset_id) DO UPDATE"));
    }

    #[test]
    fn several_instances_of_one_attribute_bind_a_single_catalogue_row() {
        // The regression: POST /temporal/entities with three instances of one
        // attribute produced three identical catalogue keys in one
        // INSERT ... ON CONFLICT DO UPDATE, which PostgreSQL rejects with
        // 21000 -- failing the whole request, not just the attribute.
        let entries = [
            tea_entry(SPEED, None, ValueType::Number),
            tea_entry(SPEED, None, ValueType::Number),
            tea_entry(SPEED, None, ValueType::Number),
        ];

        let binds = tea_batch_binds(&entries, &name_ids()).unwrap_or_default();

        assert_eq!(binds.entity_ids, vec![ENTITY]);
        assert_eq!(binds.attribute_name_ids, vec![1i16]);
        assert_eq!(binds.dataset_ids, vec![None]);
        assert_eq!(binds.value_types, vec!["Number"]);
    }

    #[test]
    fn the_last_entry_for_a_key_describes_the_bound_catalogue_row() {
        let entries = [tea_entry(SPEED, None, ValueType::Number), tea_entry(SPEED, None, ValueType::String)];

        let binds = tea_batch_binds(&entries, &name_ids()).unwrap_or_default();

        assert_eq!(binds.value_types, vec!["String"]);
    }

    #[test]
    fn entries_differing_by_attribute_or_dataset_stay_separate_rows() {
        // Distinct attribute names, and distinct dataset ids under one name, are
        // distinct catalogue rows. `dataset_id` is UNIQUE NULLS NOT DISTINCT
        // (migration 0007), so None collides with None and must collapse -- the
        // first two entries here are one row, not two.
        let entries = [
            tea_entry(SPEED, None, ValueType::Number),
            tea_entry(SPEED, None, ValueType::Number),
            tea_entry(SPEED, Some("urn:ngsi-ld:Dataset:gps"), ValueType::Number),
            tea_entry(BRAND, None, ValueType::String),
        ];

        let binds = tea_batch_binds(&entries, &name_ids()).unwrap_or_default();

        assert_eq!(binds.attribute_name_ids, vec![1i16, 1i16, 2i16]);
        assert_eq!(binds.dataset_ids, vec![None, Some("urn:ngsi-ld:Dataset:gps"), None]);
        assert_eq!(binds.value_types, vec!["Number", "Number", "String"]);
    }

    #[test]
    fn distinct_attribute_names_are_deduplicated_in_first_seen_order() {
        let entries = [
            tea_entry(BRAND, None, ValueType::String),
            tea_entry(SPEED, None, ValueType::Number),
            tea_entry(BRAND, Some("urn:ngsi-ld:Dataset:vin"), ValueType::String),
        ];

        assert_eq!(distinct_attribute_names(&entries), vec![BRAND, SPEED]);
    }

    #[test]
    fn an_attribute_missing_from_the_dictionary_is_an_internal_error() {
        let entries = [tea_entry("https://uri.etsi.org/ngsi-ld/default-context/unresolved", None, ValueType::Number)];

        assert!(tea_batch_binds(&entries, &name_ids()).is_err());
    }
}
