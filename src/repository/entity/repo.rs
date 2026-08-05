//! SQL-only repository for entity persistence.

use crate::{
    core::error::AndromedaError,
    repository::{
        entity::{
            params::{EntityCreateParams, EntitySaveParams, PreparedEntityColumns},
            rows::{AttributeSummaryRow, DeletedEntity, PurgedEntity, QueryResult},
        },
        ewkb_binding::geometry_bytes,
    },
};
use andromeda_domain::notification::SysAttrsSnapshot;
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use andromeda_query::{
    entity::{compiled::CompiledEntityQuery, sql as entity_sql},
    filter::sql::attribute_names::ResolvedAttributeNames,
    sql::bind::BindValue,
};
use serde_json::Value;
use sqlx::{Executor, Postgres, Row, Transaction, postgres::PgPool};
use std::collections::HashMap;
use urn_rs::Urn;

/// Expands to the `INSERT INTO entities` statement shared by
/// [`EntityRepository::create_in_tx`] and
/// [`EntityRepository::ensure_placeholder_in_tx`], with no conflict clause.
///
/// A macro rather than a `const` because the two statements differ only by a
/// trailing clause and `concat!` composes literals, not constants. Hand-copying
/// the column list into both would let a new column reach one statement and
/// silently miss the other.
macro_rules! entity_insert_sql {
    () => {
        r#"
            INSERT INTO entities (id, types, scope_paths, payload, location, observation_space, operation_space, expires_at)
            VALUES ($1, $2, $3::ltree[], $4,
                    CASE WHEN $5 IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB($5)) ELSE NULL END,
                    CASE WHEN $6 IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB($6)) ELSE NULL END,
                    CASE WHEN $7 IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB($7)) ELSE NULL END,
                    $8)
            "#
    };
}

/// Plain entity INSERT. Must stay free of any conflict clause: the Entity API
/// answers 409 off its duplicate-key error (NGSI-LD 6.4.3.1).
const ENTITY_INSERT_SQL: &str = entity_insert_sql!();

/// [`ENTITY_INSERT_SQL`] made idempotent, for callers that need the row to exist
/// rather than to have created it. See
/// [`EntityRepository::ensure_placeholder_in_tx`].
const ENTITY_PLACEHOLDER_INSERT_SQL: &str = concat!(entity_insert_sql!(), "ON CONFLICT (id) DO NOTHING\n");

/// Repository for persisting and querying NGSI-LD entities.
#[derive(Debug)]
pub struct EntityRepository {
    pool: PgPool,
}

impl EntityRepository {
    /// Create a new entity repository backed by the given connection pool.
    pub fn new(pool: &PgPool) -> EntityRepository {
        EntityRepository { pool: pool.clone() } // clone: Arc ref-count bump
    }

    /// Insert a new entity and return its URN identifier, inside the caller's
    /// transaction.
    ///
    /// Deliberately has no conflict clause: the Entity API answers 409 off this
    /// statement's duplicate-key error (NGSI-LD 6.4.3.1). Callers that only need
    /// the row to be present want [`Self::ensure_placeholder_in_tx`] instead.
    pub async fn create_in_tx(tx: &mut Transaction<'_, Postgres>, params: EntityCreateParams<'_>) -> Result<Urn, AndromedaError> {
        tracing::info!("Storing entity {} with types: {:?}", params.id, params.types);
        sqlx::query(ENTITY_INSERT_SQL)
            .bind(params.id)
            .bind(params.types)
            .bind(&params.scope_paths)
            .bind(params.payload)
            .bind(geometry_bytes(params.location.as_ref())?)
            .bind(geometry_bytes(params.observation_space.as_ref())?)
            .bind(geometry_bytes(params.operation_space.as_ref())?)
            .bind(params.expires_at)
            .execute(&mut **tx)
            .await?;

        Ok(params.id.parse::<Urn>()?)
    }

    /// Insert the `entities` row a temporal write depends on, leaving any
    /// existing row untouched.
    ///
    /// `temporal_entity_attributes.entity_id` has a foreign key to
    /// `entities(id)` (migration 0007), so the Temporal API has to materialise a
    /// placeholder row before it can record history for an entity the Entity API
    /// has never seen. Its semantics are *ensure the row exists*, not *create
    /// it*: two concurrent `POST /temporal/entities` requests for the same new
    /// identifier are both legitimate and neither should fail.
    ///
    /// This is deliberately a separate statement from [`Self::create_in_tx`]
    /// rather than an added `ON CONFLICT` clause on the shared one: the Entity
    /// API answers 409 off that statement's duplicate-key error (NGSI-LD
    /// 6.4.3.1), so suppressing the conflict there would silence a required
    /// response.
    pub async fn ensure_placeholder_in_tx(tx: &mut Transaction<'_, Postgres>, params: EntityCreateParams<'_>) -> Result<(), AndromedaError> {
        sqlx::query(ENTITY_PLACEHOLDER_INSERT_SQL)
            .bind(params.id)
            .bind(params.types)
            .bind(&params.scope_paths)
            .bind(params.payload)
            .bind(geometry_bytes(params.location.as_ref())?)
            .bind(geometry_bytes(params.observation_space.as_ref())?)
            .bind(geometry_bytes(params.operation_space.as_ref())?)
            .bind(params.expires_at)
            .execute(&mut **tx)
            .await?;

        Ok(())
    }

    /// Retrieve an entity's expanded payload by identifier.
    pub async fn get(&self, id: &str) -> Result<Option<Value>, AndromedaError> {
        let result: Option<Value> = sqlx::query_scalar("SELECT payload FROM entities WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;

        Ok(result)
    }

    /// Transactional variant of [`Self::get`].
    ///
    /// Lets a read-modify-write sequence take its "before" snapshot inside the
    /// same transaction that writes the result, so a concurrent writer cannot
    /// slip between the read and the write and have its attributes tombstoned
    /// by a diff computed against a stale payload.
    pub async fn get_in_tx(tx: &mut Transaction<'_, Postgres>, id: &str) -> Result<Option<Value>, AndromedaError> {
        let result: Option<Value> = sqlx::query_scalar("SELECT payload FROM entities WHERE id = $1")
            .bind(id)
            .fetch_optional(&mut **tx)
            .await?;

        Ok(result)
    }

    /// Fetch expanded payloads for multiple entities inside the caller's
    /// transaction. See [`Self::get_in_tx`] for why the read is transactional.
    pub async fn get_batch_in_tx(tx: &mut Transaction<'_, Postgres>, ids: &[String]) -> Result<HashMap<String, Value>, AndromedaError> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let rows = sqlx::query("SELECT id, payload FROM entities WHERE id = ANY($1)")
            .bind(ids)
            .fetch_all(&mut **tx)
            .await?;

        let mut result = HashMap::with_capacity(ids.len());
        for row in rows {
            let id: String = row.get("id");
            let payload: Value = row.get("payload");
            result.insert(id, payload);
        }
        Ok(result)
    }

    /// Retrieve system attributes for a single entity.
    pub async fn get_sys_attrs(&self, id: &str) -> Result<Option<SysAttrsSnapshot>, AndromedaError> {
        let row = sqlx::query("SELECT created_at, modified_at, expires_at FROM entities WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.map(|row| SysAttrsSnapshot {
            created_at: row.get("created_at"),
            modified_at: row.get("modified_at"),
            expires_at: row.get("expires_at"),
        }))
    }

    /// Delete entities whose expires_at has passed. Returns the IDs of deleted entities.
    pub async fn expire_due_entities(&self) -> Result<Vec<String>, AndromedaError> {
        let rows: Vec<(String,)> = sqlx::query_as("DELETE FROM entities WHERE expires_at IS NOT NULL AND expires_at <= NOW() RETURNING id")
            .fetch_all(&self.pool)
            .await?;

        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    /// Narrow persistence primitive: invoke the DB-side
    /// `remove_expired_attribute_instances` SQL function across every entity
    /// whose payload carries at least one past-due attribute instance, and
    /// return the set of affected entity IDs.
    ///
    /// The *when* (scheduling cadence, which entities) is decided by the
    /// service/worker layer; the *what* (the filter rule: past-due expiresAt)
    /// lives in the SQL function definition, so the repository only supplies
    /// the narrow trigger and SQL wiring.
    ///
    /// Returns the payload both before and after the rewrite for every
    /// affected entity. The caller needs the "before" side to work out which
    /// attribute instances expired, so it can record the matching tombstones
    /// in this same transaction — an expiring attribute is a deletion, and
    /// without the tombstone the temporal history and the `attribute_current`
    /// projection keep serving a value the payload no longer has.
    ///
    /// Both payloads come from one statement, and therefore one snapshot: the
    /// `candidates` CTE reads the pre-update rows while the `purged` CTE
    /// writes them, so the pair is guaranteed consistent without a second
    /// round trip or an explicit row lock.
    pub async fn purge_expired_attributes_in_tx(tx: &mut Transaction<'_, Postgres>) -> Result<Vec<PurgedEntity>, AndromedaError> {
        let location_iri = properties::LOCATION.as_str();
        let observation_space_iri = properties::OBSERVATION_SPACE.as_str();
        let operation_space_iri = properties::OPERATION_SPACE.as_str();

        let rows = sqlx::query(
            r#"
            WITH candidates AS (
                SELECT id, payload AS previous_payload
                FROM entities
                WHERE EXISTS (
                    SELECT 1
                    FROM (
                        SELECT key, value
                        FROM jsonb_each(payload->0)
                        WHERE key NOT LIKE '@%' AND jsonb_typeof(value) = 'array'
                    ) AS kv(key, val),
                    LATERAL jsonb_array_elements(kv.val) AS inst(elem)
                    WHERE (inst.elem #>>'{https://uri.etsi.org/ngsi-ld/expiresAt,0,@value}')::timestamptz <= NOW()
                )
            ),
            purged AS (
                UPDATE entities
                SET payload = remove_expired_attribute_instances(entities.payload, NOW()),
                    modified_at = NOW(),
                    location = CASE
                        WHEN remove_expired_attribute_instances(entities.payload, NOW())->0 ? $1
                        THEN entities.location
                        ELSE NULL
                    END,
                    observation_space = CASE
                        WHEN remove_expired_attribute_instances(entities.payload, NOW())->0 ? $2
                        THEN entities.observation_space
                        ELSE NULL
                    END,
                    operation_space = CASE
                        WHEN remove_expired_attribute_instances(entities.payload, NOW())->0 ? $3
                        THEN entities.operation_space
                        ELSE NULL
                    END
                FROM candidates c
                WHERE entities.id = c.id
                RETURNING entities.id, entities.payload
            )
            SELECT p.id, c.previous_payload, p.payload
            FROM purged p
            JOIN candidates c ON c.id = p.id
            "#,
        )
        .bind(location_iri)
        .bind(observation_space_iri)
        .bind(operation_space_iri)
        .fetch_all(&mut **tx)
        .await?;

        let purged = rows
            .into_iter()
            .map(|row| PurgedEntity {
                id: row.get("id"),
                previous_payload: row.get("previous_payload"),
                payload: row.get("payload"),
            })
            .collect();
        Ok(purged)
    }

    /// Overwrite an existing entity with pre-extracted columns, inside the
    /// caller's transaction. Returns rows affected.
    ///
    /// Transaction-only by design: every payload overwrite has to be paired
    /// with the temporal records and tombstones that describe it, so there is
    /// no pool-level variant that would let a caller write the payload alone.
    pub async fn save_in_tx(tx: &mut Transaction<'_, Postgres>, params: EntitySaveParams<'_>) -> Result<u64, AndromedaError> {
        Self::save_impl(&mut **tx, params).await
    }

    async fn save_impl<'e>(executor: impl Executor<'e, Database = sqlx::Postgres>, params: EntitySaveParams<'_>) -> Result<u64, AndromedaError> {
        let result = sqlx::query(
            r#"
            UPDATE entities
            SET payload = $2, types = $3, scope_paths = $4::ltree[],
                location = CASE WHEN $5 IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB($5)) ELSE NULL END,
                observation_space = CASE WHEN $6 IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB($6)) ELSE NULL END,
                operation_space = CASE WHEN $7 IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB($7)) ELSE NULL END,
                modified_at = NOW(),
                expires_at = COALESCE($8, expires_at)
            WHERE id = $1
            "#,
        )
        .bind(params.id)
        .bind(params.payload)
        // The column is `text[]`; the domain name type is not an sqlx `Encode`, so it is rendered here at the persistence boundary.
        .bind(params.types.iter().map(ToString::to_string).collect::<Vec<_>>())
        .bind(&params.scope_paths)
        .bind(geometry_bytes(params.location.as_ref())?)
        .bind(geometry_bytes(params.observation_space.as_ref())?)
        .bind(geometry_bytes(params.operation_space.as_ref())?)
        .bind(params.expires_at)
        .execute(executor)
        .await?;

        Ok(result.rows_affected())
    }

    /// Transactional delete of a single entity by id. Returns rows affected.
    pub async fn delete_in_tx(tx: &mut Transaction<'_, Postgres>, id: &str) -> Result<u64, AndromedaError> {
        let result = sqlx::query("DELETE FROM entities WHERE id = $1").bind(id).execute(&mut **tx).await?;
        Ok(result.rows_affected())
    }

    /// Execute a compiled entity query and return matching payloads.
    pub async fn query_compiled(&self, compiled: &CompiledEntityQuery, attribute_names: &ResolvedAttributeNames) -> Result<QueryResult, AndromedaError> {
        let built = entity_sql::build_query(compiled, attribute_names)?;

        tracing::debug!("Built SQL: {} with binds: {:?}", built.sql, built.bind_values);

        let (rows, window_count) = self.execute_dynamic_query(&built.sql, &built.bind_values, built.count_sql.is_some()).await?;

        let total_count = match (&built.count_sql, window_count, rows.is_empty()) {
            (Some(_), Some(count), _) => Some(count),
            (Some(count_sql), None, true) => Some(self.execute_count_query(count_sql, &built.bind_values).await?),
            (Some(_), None, false) | (None, _, _) => None,
        };

        Ok(QueryResult { rows, total_count })
    }

    async fn execute_dynamic_query(&self, sql: &str, binds: &[BindValue<'_>], read_total_count: bool) -> Result<(Vec<Value>, Option<i64>), AndromedaError> {
        let query = crate::bind_values!(sqlx::query(sql), binds);

        let rows = query.fetch_all(&self.pool).await?;
        let mut row_iter = rows.into_iter();
        let total_count = if read_total_count {
            match row_iter.next() {
                Some(first) => {
                    let count = first.try_get::<i64, _>(entity_sql::TOTAL_COUNT_COLUMN).ok();
                    let data: Value = first.try_get("payload")?;
                    let mut results = Vec::with_capacity(row_iter.size_hint().0 + 1);
                    results.push(data);
                    for row in row_iter {
                        results.push(row.try_get::<Value, _>("payload")?);
                    }
                    return Ok((results, count));
                }
                None => None,
            }
        } else {
            None
        };
        let results: Vec<Value> = row_iter.map(|row| row.try_get::<Value, _>("payload")).collect::<Result<_, _>>()?;
        Ok((results, total_count))
    }

    async fn execute_count_query(&self, sql: &str, binds: &[BindValue<'_>]) -> Result<i64, AndromedaError> {
        let query = crate::bind_values!(sqlx::query_scalar(sql), binds);

        let count: i64 = query.fetch_one(&self.pool).await?;
        Ok(count)
    }

    /// Fetch sys_attrs for multiple entities in one query.
    pub async fn get_sys_attrs_batch(&self, ids: &[String]) -> Result<HashMap<String, SysAttrsSnapshot>, AndromedaError> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let rows = sqlx::query("SELECT id, created_at, modified_at, expires_at FROM entities WHERE id = ANY($1)")
            .bind(ids)
            .fetch_all(&self.pool)
            .await?;

        let result = rows
            .into_iter()
            .map(|row| {
                let id: String = row.get("id");
                (
                    id,
                    SysAttrsSnapshot {
                        created_at: row.get("created_at"),
                        modified_at: row.get("modified_at"),
                        expires_at: row.get("expires_at"),
                    },
                )
            })
            .collect();
        Ok(result)
    }

    /// Fetch expanded payloads for multiple entities in one query.
    pub async fn get_batch(&self, ids: &[String]) -> Result<HashMap<String, Value>, AndromedaError> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let rows = sqlx::query("SELECT id, payload FROM entities WHERE id = ANY($1)")
            .bind(ids)
            .fetch_all(&self.pool)
            .await?;

        let mut result = HashMap::with_capacity(ids.len());
        for row in rows {
            let id: String = row.get("id");
            let payload: Value = row.get("payload");
            result.insert(id, payload);
        }
        Ok(result)
    }

    /// Check whether an entity with the given identifier exists.
    pub async fn exists(&self, id: &str) -> Result<bool, AndromedaError> {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM entities WHERE id = $1)")
            .bind(id)
            .fetch_one(&self.pool)
            .await?;

        Ok(exists)
    }

    /// Return all distinct entity type IRIs present in the store.
    pub async fn types(&self) -> Result<Vec<String>, AndromedaError> {
        Ok(sqlx::query_scalar("SELECT DISTINCT unnest(types) FROM entities").fetch_all(&self.pool).await?)
    }

    /// Retrieve entity payloads filtered by type with optional pagination.
    pub async fn get_entities_by_type(&self, type_: &str, limit: Option<i64>, offset: Option<i64>) -> Result<Vec<Value>, AndromedaError> {
        Ok(
            sqlx::query_scalar("SELECT payload FROM entities WHERE $1 = ANY(types) ORDER BY id LIMIT $2 OFFSET $3")
                .bind(type_)
                .bind(limit)
                .bind(offset.unwrap_or(0))
                .fetch_all(&self.pool)
                .await?,
        )
    }

    /// Retrieve all entity payloads with optional pagination.
    pub async fn get_all_entities(&self, limit: Option<i64>, offset: Option<i64>) -> Result<Vec<Value>, AndromedaError> {
        Ok(sqlx::query_scalar("SELECT payload FROM entities ORDER BY id LIMIT $1 OFFSET $2")
            .bind(limit)
            .bind(offset.unwrap_or(0))
            .fetch_all(&self.pool)
            .await?)
    }

    /// Get per-type attribute summaries using SQL aggregation, with keyset
    /// pagination on the aggregated `type_iri` key.
    ///
    /// Query plan assumption: the server materialises the full aggregation
    /// (one row per distinct type) before applying the keyset filter + LIMIT,
    /// so this pagination caps transport volume rather than server-side work.
    /// It exists to bound memory for the HTTP response; it does not improve
    /// the underlying aggregate cost.
    pub async fn get_entity_type_summaries(&self, after: Option<&str>, limit: u32) -> Result<Vec<(String, Vec<String>)>, AndromedaError> {
        let rows = sqlx::query(
            r#"
            SELECT t AS type_iri,
                   COALESCE(array_agg(DISTINCT attr_key) FILTER (WHERE attr_key IS NOT NULL), '{}') AS attr_keys
            FROM entities,
                 unnest(types) AS t,
                 LATERAL jsonb_object_keys(
                     CASE jsonb_typeof(payload->0) WHEN 'object' THEN payload->0 ELSE payload END
                 ) AS attr_key
            WHERE attr_key NOT LIKE '@%'
            GROUP BY t
            HAVING $1::text IS NULL OR t > $1
            ORDER BY t
            LIMIT $2
            "#,
        )
        .bind(after)
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await?;

        let results = rows
            .into_iter()
            .map(|row| {
                let type_iri: String = row.get("type_iri");
                let attr_keys: Vec<String> = row.get("attr_keys");
                (type_iri, attr_keys)
            })
            .collect();
        Ok(results)
    }

    /// Get attribute metadata aggregated at the SQL level with keyset
    /// pagination on `attr_key` (see [`Self::get_entity_type_summaries`] for
    /// query-plan assumptions).
    pub async fn get_attribute_summary(&self, after: Option<&str>, limit: u32) -> Result<Vec<AttributeSummaryRow>, AndromedaError> {
        let rows = sqlx::query(
            r#"
            SELECT attr_key,
                   COUNT(DISTINCT e.id) AS entity_count,
                   array_agg(DISTINCT t) AS entity_types,
                   array_agg(DISTINCT at) FILTER (WHERE at IS NOT NULL) AS attribute_types
            FROM entities e,
                 LATERAL (
                     SELECT key AS attr_key, value AS attr_value
                     FROM jsonb_each(
                         CASE jsonb_typeof(e.payload->0) WHEN 'object' THEN e.payload->0 ELSE e.payload END
                     )
                     WHERE key NOT LIKE '@%'
                 ) attrs,
                 unnest(e.types) AS t
                 LEFT JOIN LATERAL jsonb_array_elements_text(attrs.attr_value->0->'@type') AS at ON true
            GROUP BY attr_key
            HAVING $1::text IS NULL OR attr_key > $1
            ORDER BY attr_key
            LIMIT $2
            "#,
        )
        .bind(after)
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await?;

        let results = rows
            .into_iter()
            .map(|row| {
                let attribute_types: Option<Vec<String>> = row.get("attribute_types");
                AttributeSummaryRow {
                    attr_iri: row.get("attr_key"),
                    entity_count: row.get("entity_count"),
                    entity_types: row.get("entity_types"),
                    attribute_types: attribute_types.unwrap_or_default(),
                }
            })
            .collect();
        Ok(results)
    }

    /// Fetch attribute summary for a single attribute IRI.
    pub async fn get_attribute_info(&self, attr_iri: &str) -> Result<Option<AttributeSummaryRow>, AndromedaError> {
        let row = sqlx::query(
            r#"
            SELECT attr_key,
                   COUNT(DISTINCT e.id) AS entity_count,
                   array_agg(DISTINCT t) AS entity_types,
                   array_agg(DISTINCT at) FILTER (WHERE at IS NOT NULL) AS attribute_types
            FROM entities e,
                 LATERAL (
                     SELECT key AS attr_key, value AS attr_value
                     FROM jsonb_each(
                         CASE jsonb_typeof(e.payload->0) WHEN 'object' THEN e.payload->0 ELSE e.payload END
                     )
                     WHERE key = $1
                 ) attrs,
                 unnest(e.types) AS t
                 LEFT JOIN LATERAL jsonb_array_elements_text(attrs.attr_value->0->'@type') AS at ON true
            GROUP BY attr_key
            "#,
        )
        .bind(attr_iri)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => {
                let attribute_types: Option<Vec<String>> = row.get("attribute_types");
                Ok(Some(AttributeSummaryRow {
                    attr_iri: row.get("attr_key"),
                    entity_count: row.get("entity_count"),
                    entity_types: row.get("entity_types"),
                    attribute_types: attribute_types.unwrap_or_default(),
                }))
            }
            None => Ok(None),
        }
    }

    /// Execute a compiled query and return only the matching entity identifiers.
    pub async fn query_ids(&self, compiled: &CompiledEntityQuery, attribute_names: &ResolvedAttributeNames) -> Result<Vec<String>, AndromedaError> {
        let built = entity_sql::build_ids_query(compiled, attribute_names)?;

        tracing::debug!("Built IDs SQL: {} with binds: {:?}", built.sql, built.bind_values);

        let query = crate::bind_values!(sqlx::query(&built.sql), &built.bind_values);

        let rows = query.fetch_all(&self.pool).await?;
        let ids: Vec<String> = rows.iter().map(|row| row.get("id")).collect();
        Ok(ids)
    }

    /// Count entities that have the given type IRI.
    pub async fn count_by_type(&self, type_: &str) -> Result<i64, AndromedaError> {
        Ok(sqlx::query_scalar("SELECT COUNT(*) FROM entities WHERE $1 = ANY(types)")
            .bind(type_)
            .fetch_one(&self.pool)
            .await?)
    }

    /// Insert multiple entities in a single SQL statement using `UNNEST`.
    /// Lets callers pair the entity insert with another write (e.g.
    /// enqueueing a side-effect job) inside a single commit.
    pub async fn create_batch_in_tx(tx: &mut Transaction<'_, Postgres>, entities: &PreparedEntityColumns) -> Result<Vec<String>, AndromedaError> {
        Self::create_batch_impl(&mut **tx, entities).await
    }

    async fn create_batch_impl<'e>(executor: impl Executor<'e, Database = Postgres>, entities: &PreparedEntityColumns) -> Result<Vec<String>, AndromedaError> {
        if entities.ids.is_empty() {
            return Ok(Vec::new());
        }

        let rows: Vec<(String,)> = sqlx::query_as(
            r#"
            INSERT INTO entities (id, types, scope_paths, payload, location, observation_space, operation_space, expires_at)
            SELECT
                d.id,
                ARRAY(SELECT jsonb_array_elements_text(d.types_json)),
                ARRAY(SELECT jsonb_array_elements_text(d.scope_paths_json)::ltree),
                d.payload::jsonb,
                CASE WHEN d.loc IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB(d.loc)) ELSE NULL END,
                CASE WHEN d.obs IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB(d.obs)) ELSE NULL END,
                CASE WHEN d.ops IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB(d.ops)) ELSE NULL END,
                d.expires_at
            FROM UNNEST($1::text[], $2::jsonb[], $3::jsonb[], $4::text[], $5::bytea[], $6::bytea[], $7::bytea[], $8::timestamptz[])
                AS d(id, types_json, scope_paths_json, payload, loc, obs, ops, expires_at)
            ON CONFLICT (id) DO NOTHING
            RETURNING id
            "#,
        )
        .bind(&entities.ids)
        .bind(&entities.types_json)
        .bind(&entities.scope_paths_json)
        .bind(&entities.payload_json)
        .bind(&entities.locations)
        .bind(&entities.obs_spaces)
        .bind(&entities.op_spaces)
        .bind(&entities.expires_ats)
        .fetch_all(executor)
        .await?;

        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    /// Delete multiple entities in a single SQL statement.
    pub async fn delete_batch(&self, ids: &[String]) -> Result<Vec<DeletedEntity>, AndromedaError> {
        Self::delete_batch_impl(&self.pool, ids).await
    }

    /// Transactional variant of [`Self::delete_batch`].
    pub async fn delete_batch_in_tx(tx: &mut Transaction<'_, Postgres>, ids: &[String]) -> Result<Vec<DeletedEntity>, AndromedaError> {
        Self::delete_batch_impl(&mut **tx, ids).await
    }

    async fn delete_batch_impl<'e>(executor: impl Executor<'e, Database = Postgres>, ids: &[String]) -> Result<Vec<DeletedEntity>, AndromedaError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let rows = sqlx::query("DELETE FROM entities WHERE id = ANY($1) RETURNING id, payload, created_at, modified_at, expires_at")
            .bind(ids)
            .fetch_all(executor)
            .await?;

        let result = rows
            .into_iter()
            .map(|row| DeletedEntity {
                id: row.get("id"),
                payload: row.get("payload"),
                created_at: row.get("created_at"),
                modified_at: row.get("modified_at"),
                expires_at: row.get("expires_at"),
            })
            .collect();
        Ok(result)
    }

    /// Update multiple existing entities in a single SQL statement using
    /// `UNNEST`, inside the caller's transaction.
    pub async fn save_batch_in_tx(tx: &mut Transaction<'_, Postgres>, entities: &PreparedEntityColumns) -> Result<Vec<String>, AndromedaError> {
        Self::save_batch_impl(&mut **tx, entities).await
    }

    async fn save_batch_impl<'e>(executor: impl Executor<'e, Database = Postgres>, entities: &PreparedEntityColumns) -> Result<Vec<String>, AndromedaError> {
        if entities.ids.is_empty() {
            return Ok(Vec::new());
        }

        let rows: Vec<(String,)> = sqlx::query_as(
            r#"
            UPDATE entities SET
                payload = d.payload::jsonb,
                types = ARRAY(SELECT jsonb_array_elements_text(d.types_json)),
                scope_paths = ARRAY(SELECT jsonb_array_elements_text(d.scope_paths_json)::ltree),
                location = CASE WHEN d.loc IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB(d.loc)) ELSE NULL END,
                observation_space = CASE WHEN d.obs IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB(d.obs)) ELSE NULL END,
                operation_space = CASE WHEN d.ops IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB(d.ops)) ELSE NULL END,
                modified_at = NOW(),
                expires_at = COALESCE(d.expires_at, entities.expires_at)
            FROM UNNEST($1::text[], $2::jsonb[], $3::jsonb[], $4::text[], $5::bytea[], $6::bytea[], $7::bytea[], $8::timestamptz[])
                AS d(id, types_json, scope_paths_json, payload, loc, obs, ops, expires_at)
            WHERE entities.id = d.id
            RETURNING entities.id
            "#,
        )
        .bind(&entities.ids)
        .bind(&entities.types_json)
        .bind(&entities.scope_paths_json)
        .bind(&entities.payload_json)
        .bind(&entities.locations)
        .bind(&entities.obs_spaces)
        .bind(&entities.op_spaces)
        .bind(&entities.expires_ats)
        .fetch_all(executor)
        .await?;

        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    /// Insert or update multiple entities in a single SQL statement inside
    /// the caller's transaction.
    pub async fn upsert_batch_in_tx(tx: &mut Transaction<'_, Postgres>, entities: &PreparedEntityColumns) -> Result<Vec<(String, bool)>, AndromedaError> {
        Self::upsert_batch_impl(&mut **tx, entities).await
    }

    async fn upsert_batch_impl<'e>(
        executor: impl Executor<'e, Database = Postgres>,
        entities: &PreparedEntityColumns,
    ) -> Result<Vec<(String, bool)>, AndromedaError> {
        if entities.ids.is_empty() {
            return Ok(Vec::new());
        }

        let rows = sqlx::query(
            r#"
            INSERT INTO entities (id, types, scope_paths, payload, location, observation_space, operation_space, expires_at)
            SELECT
                d.id,
                ARRAY(SELECT jsonb_array_elements_text(d.types_json)),
                ARRAY(SELECT jsonb_array_elements_text(d.scope_paths_json)::ltree),
                d.payload::jsonb,
                CASE WHEN d.loc IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB(d.loc)) ELSE NULL END,
                CASE WHEN d.obs IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB(d.obs)) ELSE NULL END,
                CASE WHEN d.ops IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB(d.ops)) ELSE NULL END,
                d.expires_at
            FROM UNNEST($1::text[], $2::jsonb[], $3::jsonb[], $4::text[], $5::bytea[], $6::bytea[], $7::bytea[], $8::timestamptz[])
                AS d(id, types_json, scope_paths_json, payload, loc, obs, ops, expires_at)
            ON CONFLICT (id) DO UPDATE SET
                payload = EXCLUDED.payload,
                types = EXCLUDED.types,
                scope_paths = EXCLUDED.scope_paths,
                location = EXCLUDED.location,
                observation_space = EXCLUDED.observation_space,
                operation_space = EXCLUDED.operation_space,
                modified_at = NOW(),
                expires_at = COALESCE(EXCLUDED.expires_at, entities.expires_at)
            RETURNING id, (xmax = 0) AS was_created
            "#,
        )
        .bind(&entities.ids)
        .bind(&entities.types_json)
        .bind(&entities.scope_paths_json)
        .bind(&entities.payload_json)
        .bind(&entities.locations)
        .bind(&entities.obs_spaces)
        .bind(&entities.op_spaces)
        .bind(&entities.expires_ats)
        .fetch_all(executor)
        .await?;

        let result = rows
            .into_iter()
            .map(|row| {
                let id: String = row.get("id");
                let was_created: bool = row.get("was_created");
                (id, was_created)
            })
            .collect();
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_entity_insert_carries_no_conflict_clause() {
        // The Entity API answers 409 off this statement's duplicate-key error
        // (NGSI-LD 6.4.3.1). A conflict clause here would swallow it and turn a
        // duplicate create into a silent success.
        assert!(!ENTITY_INSERT_SQL.contains("ON CONFLICT"));
    }

    #[test]
    fn placeholder_insert_is_the_plain_insert_plus_a_do_nothing_clause() {
        // Derived from the shared statement rather than copied, so a column
        // added for one reaches the other and the only difference between the
        // two stays the conflict clause.
        pretty_assertions::assert_eq!(
            ENTITY_PLACEHOLDER_INSERT_SQL.strip_prefix(ENTITY_INSERT_SQL).map(str::trim),
            Some("ON CONFLICT (id) DO NOTHING")
        );
    }
}
