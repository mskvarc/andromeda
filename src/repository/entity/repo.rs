//! SQL-only repository for entity persistence.

use crate::{
    core::error::AndromedaError,
    domain::{notification::SysAttrsSnapshot, outbox::NewOutboxEntry},
    query::{
        compiled::CompiledEntityQuery,
        sql::{self, BindValue},
    },
    repository::{
        entity::{
            params::{EntityCreateParams, EntitySaveParams, PreparedEntityColumns},
            rows::{AttributeSummaryRow, DeletedEntity, QueryResult},
        },
        outbox::repo::OutboxRepository,
    },
};
use serde_json::Value;
use sqlx::{Executor, Postgres, Row, Transaction, postgres::PgPool};
use std::collections::HashMap;
use urn_rs::Urn;

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

    /// Insert a new entity and return its URN identifier.
    pub async fn create(&self, params: EntityCreateParams<'_>) -> Result<Urn, AndromedaError> {
        Self::create_impl(&self.pool, params).await
    }

    /// Insert a new entity atomically with outbox entries, returning the URN.
    pub async fn create_with_outbox(&self, params: EntityCreateParams<'_>, outbox_entries: &[NewOutboxEntry]) -> Result<Urn, AndromedaError> {
        let mut tx = self.pool.begin().await?;
        let urn = Self::create_impl(&mut *tx, params).await?;
        OutboxRepository::insert_batch_in_tx(&mut tx, outbox_entries).await?;
        tx.commit().await?;
        Ok(urn)
    }

    async fn create_impl<'e>(executor: impl Executor<'e, Database = sqlx::Postgres>, params: EntityCreateParams<'_>) -> Result<Urn, AndromedaError> {
        tracing::info!("Storing entity {} with types: {:?}", params.id, params.types);
        sqlx::query(
            r#"
            INSERT INTO entities (id, types, scope_paths, payload, location, observation_space, operation_space, expires_at)
            VALUES ($1, $2, $3::ltree[], $4,
                    CASE WHEN $5 IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB($5)) ELSE NULL END,
                    CASE WHEN $6 IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB($6)) ELSE NULL END,
                    CASE WHEN $7 IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB($7)) ELSE NULL END,
                    $8)
            "#,
        )
        .bind(params.id)
        .bind(params.types)
        .bind(&params.scope_paths)
        .bind(params.payload)
        .bind(params.location_wkb)
        .bind(params.observation_space_wkb)
        .bind(params.operation_space_wkb)
        .bind(params.expires_at)
        .execute(executor)
        .await?;

        Ok(params.id.parse::<Urn>()?)
    }

    /// Retrieve an entity's expanded payload by identifier.
    pub async fn get(&self, id: &str) -> Result<Option<Value>, AndromedaError> {
        let result: Option<Value> = sqlx::query_scalar("SELECT payload FROM entities WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;

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
    pub async fn purge_expired_attributes(&self) -> Result<Vec<String>, AndromedaError> {
        let location_iri = crate::jsonld::vocab::expanded::LOCATION.as_str();
        let observation_space_iri = crate::jsonld::vocab::expanded::OBSERVATION_SPACE.as_str();
        let operation_space_iri = crate::jsonld::vocab::expanded::OPERATION_SPACE.as_str();

        let rows: Vec<(String,)> = sqlx::query_as(
            r#"
            UPDATE entities
            SET payload = remove_expired_attribute_instances(payload, NOW()),
                modified_at = NOW(),
                location = CASE
                    WHEN remove_expired_attribute_instances(payload, NOW())->0 ? $1
                    THEN location
                    ELSE NULL
                END,
                observation_space = CASE
                    WHEN remove_expired_attribute_instances(payload, NOW())->0 ? $2
                    THEN observation_space
                    ELSE NULL
                END,
                operation_space = CASE
                    WHEN remove_expired_attribute_instances(payload, NOW())->0 ? $3
                    THEN operation_space
                    ELSE NULL
                END
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
            RETURNING id
            "#,
        )
        .bind(location_iri)
        .bind(observation_space_iri)
        .bind(operation_space_iri)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    /// Save (overwrite) an existing entity with pre-extracted columns.
    /// Returns rows affected.
    pub async fn save(&self, params: EntitySaveParams<'_>) -> Result<u64, AndromedaError> {
        Self::save_impl(&self.pool, params).await
    }

    /// Save an entity atomically with outbox entries. Returns rows affected.
    pub async fn save_with_outbox(&self, params: EntitySaveParams<'_>, outbox_entries: &[NewOutboxEntry]) -> Result<u64, AndromedaError> {
        let mut tx = self.pool.begin().await?;
        let updated = Self::save_impl(&mut *tx, params).await?;
        OutboxRepository::insert_batch_in_tx(&mut tx, outbox_entries).await?;
        tx.commit().await?;
        Ok(updated)
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
        .bind(&params.types)
        .bind(&params.scope_paths)
        .bind(params.location_wkb)
        .bind(params.observation_space_wkb)
        .bind(params.operation_space_wkb)
        .bind(params.expires_at)
        .execute(executor)
        .await?;

        Ok(result.rows_affected())
    }

    /// Delete an entity by its identifier. Returns rows affected.
    pub async fn delete(&self, id: &str) -> Result<u64, AndromedaError> {
        Self::delete_impl(&self.pool, id).await
    }

    /// Delete an entity atomically with outbox entries. Returns rows affected.
    pub async fn delete_with_outbox(&self, id: &str, outbox_entries: &[NewOutboxEntry]) -> Result<u64, AndromedaError> {
        let mut tx = self.pool.begin().await?;
        let deleted = Self::delete_impl(&mut *tx, id).await?;
        OutboxRepository::insert_batch_in_tx(&mut tx, outbox_entries).await?;
        tx.commit().await?;
        Ok(deleted)
    }

    async fn delete_impl<'e>(executor: impl Executor<'e, Database = sqlx::Postgres>, id: &str) -> Result<u64, AndromedaError> {
        let result = sqlx::query("DELETE FROM entities WHERE id = $1").bind(id).execute(executor).await?;

        Ok(result.rows_affected())
    }

    /// Execute a compiled entity query and return matching payloads.
    pub async fn query_compiled(&self, compiled: &CompiledEntityQuery) -> Result<QueryResult, AndromedaError> {
        let built = sql::entity::build_query(compiled)?;

        tracing::info!("Built SQL: {} with binds: {:?}", built.sql, built.bind_values);

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
                    let count = first.try_get::<i64, _>(sql::entity::TOTAL_COUNT_COLUMN).ok();
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
    pub async fn query_ids(&self, compiled: &CompiledEntityQuery) -> Result<Vec<String>, AndromedaError> {
        let built = sql::entity::build_ids_query(compiled)?;

        tracing::info!("Built IDs SQL: {} with binds: {:?}", built.sql, built.bind_values);

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

    /// Expose the underlying connection pool so services can open their
    /// own transactions (e.g. for the async durable-queue path).
    pub const fn pool(&self) -> &PgPool {
        &self.pool
    }
}
