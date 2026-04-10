//! Repository for persisting and querying NGSI-LD entities.
//!
//! Handles single-entity CRUD, batch operations (via SQL `UNNEST`), compiled
//! query execution, entity type/attribute discovery, and expiration cleanup.

use crate::{
    domain::{expanded, notification::SysAttrsSnapshot},
    error::AndromedaError,
    query::{
        compiled::CompiledEntityQuery,
        sql::{self, BindValue},
    },
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{Row, postgres::PgPool};
use std::collections::HashMap;
use urn::Urn;

/// Row data prepared for bulk entity SQL operations.
///
/// Each field corresponds to a column (or derived column) in the `entities`
/// table.  The `types_json` and `scope_paths_json` fields are JSON arrays of
/// strings because PostgreSQL `UNNEST` flattens `text[][]` into individual
/// elements rather than per-row arrays.
#[derive(Debug)]
pub struct PreparedEntityRow {
    /// Entity identifier (URN).
    pub id: String,
    /// JSON array of type IRIs (e.g. `["https://example.com/Vehicle"]`).
    pub types_json: Value,
    /// JSON array of ltree-formatted scope paths (e.g. `["Madrid", "Traffic"]`).
    pub scope_paths_json: Value,
    /// Full expanded JSON-LD payload.
    pub payload: Value,
    /// GeoJSON string for the `location` GeoProperty, if present.
    pub location: Option<String>,
    /// GeoJSON string for the `observationSpace` GeoProperty, if present.
    pub observation_space: Option<String>,
    /// GeoJSON string for the `operationSpace` GeoProperty, if present.
    pub operation_space: Option<String>,
    /// Entity expiration timestamp (ETSI GS CIM 009 clause 4.22).
    pub expires_at: Option<DateTime<Utc>>,
}

/// Entity data returned by `delete_batch` via the SQL `RETURNING` clause.
///
/// Contains the deleted entity's payload and system attributes so the caller
/// can emit change events without additional queries.
#[derive(Debug)]
pub struct DeletedEntity {
    /// Entity identifier.
    pub id: String,
    /// The expanded JSON-LD payload at the time of deletion.
    pub payload: Value,
    /// Timestamp when the entity was originally created.
    pub created_at: DateTime<Utc>,
    /// Timestamp of the last modification before deletion.
    pub modified_at: DateTime<Utc>,
    /// Optional expiration timestamp.
    pub expires_at: Option<DateTime<Utc>>,
}

/// Result of an entity query containing matched rows and an optional total count.
#[derive(Debug)]
pub struct QueryResult {
    /// The matched entity payloads.
    pub rows: Vec<Value>,
    /// Total number of matching entities, present when a count was requested.
    pub total_count: Option<i64>,
}

/// Aggregated metadata for a single attribute across all entities.
#[derive(Debug)]
pub struct AttributeSummaryRow {
    /// The expanded IRI of the attribute.
    pub attr_iri: String,
    /// Number of distinct entities that carry this attribute.
    pub entity_count: i64,
    /// Entity type IRIs that contain this attribute.
    pub entity_types: Vec<String>,
    /// NGSI-LD attribute types (Property, Relationship, etc.) observed for this attribute.
    pub attribute_types: Vec<String>,
}

/// Repository for persisting and querying NGSI-LD entities.
#[derive(Debug)]
pub struct EntityRepository {
    pool: PgPool,
}

impl EntityRepository {
    /// Create a new entity repository backed by the given connection pool.
    pub fn new(pool: &PgPool) -> EntityRepository {
        EntityRepository { pool: pool.clone() } // clone: Arc ref-count bump (PgPool is Arc-based)
    }

    /// Insert a new entity and return its URN identifier.
    pub async fn create(&self, id: &str, type_: &[&str], scopes: &[String], entity: &Value, expires_at: Option<DateTime<Utc>>) -> Result<Urn, AndromedaError> {
        tracing::info!("Storing entity {} with types: {:?}", id, type_);
        let location_geojson = expanded::extract_location_geojson(entity);
        let observation_space_geojson = expanded::extract_observation_space_geojson(entity);
        let operation_space_geojson = expanded::extract_operation_space_geojson(entity);
        let scope_paths: Vec<String> = scopes.iter().map(|s| sql::scope_to_ltree(s)).collect();
        sqlx::query(
            r#"
            INSERT INTO entities (id, types, scope_paths, payload, location, observation_space, operation_space, expires_at)
            VALUES ($1, $2, $3::ltree[], $4,
                    CASE WHEN $5 IS NOT NULL THEN ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON($5), 4326)) ELSE NULL END,
                    CASE WHEN $6 IS NOT NULL THEN ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON($6), 4326)) ELSE NULL END,
                    CASE WHEN $7 IS NOT NULL THEN ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON($7), 4326)) ELSE NULL END,
                    $8)
            "#,
        )
        .bind(id)
        .bind(type_)
        .bind(&scope_paths)
        .bind(entity)
        .bind(&location_geojson)
        .bind(&observation_space_geojson)
        .bind(&operation_space_geojson)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;

        Ok(id.parse::<Urn>()?)
    }

    /// Retrieve an entity's expanded payload by identifier, returning `None` if not found.
    pub async fn get(&self, id: &str) -> Result<Option<Value>, AndromedaError> {
        let result: Option<Value> = sqlx::query_scalar("SELECT payload FROM entities WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;

        Ok(result)
    }

    /// Retrieve system attributes (created_at, modified_at, expires_at) for a single entity.
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

    /// Remove expired attribute instances from entity payloads and return IDs of affected entities.
    ///
    /// Uses the `remove_expired_attribute_instances` SQL function to atomically
    /// filter out attribute instances whose `expiresAt` has passed (ETSI GS CIM 009
    /// clause 4.22). Also re-extracts hoisted geometry columns (`location`,
    /// `observation_space`, `operation_space`) from the cleaned payload, nulling
    /// them if the corresponding GeoProperty was entirely removed.
    pub async fn expire_due_attributes(&self) -> Result<Vec<String>, AndromedaError> {
        let location_iri = crate::jsonld::vocab::LOCATION.as_str();
        let observation_space_iri = crate::jsonld::vocab::OBSERVATION_SPACE.as_str();
        let operation_space_iri = crate::jsonld::vocab::OPERATION_SPACE.as_str();

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

    /// Save (overwrite) the expanded payload for an existing entity.
    /// Automatically re-extracts types, scopes, location, and geo spaces from the payload.
    /// Preserves existing expires_at unless the payload contains a new expiresAt value.
    pub async fn save(&self, id: &str, entity: &Value) -> Result<bool, AndromedaError> {
        let types = expanded::extract_types(entity);
        let scope_paths: Vec<String> = expanded::extract_scopes(entity).iter().map(|s| sql::scope_to_ltree(s)).collect();
        let location_geojson = expanded::extract_location_geojson(entity);
        let observation_space_geojson = expanded::extract_observation_space_geojson(entity);
        let operation_space_geojson = expanded::extract_operation_space_geojson(entity);
        let expires_at = expanded::extract_expires_at(entity);
        let result = sqlx::query(
            r#"
            UPDATE entities
            SET payload = $2, types = $3, scope_paths = $4::ltree[],
                location = CASE WHEN $5 IS NOT NULL THEN ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON($5), 4326)) ELSE NULL END,
                observation_space = CASE WHEN $6 IS NOT NULL THEN ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON($6), 4326)) ELSE NULL END,
                operation_space = CASE WHEN $7 IS NOT NULL THEN ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON($7), 4326)) ELSE NULL END,
                modified_at = NOW(),
                expires_at = COALESCE($8, expires_at)
            WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(entity)
        .bind(&types)
        .bind(&scope_paths)
        .bind(&location_geojson)
        .bind(&observation_space_geojson)
        .bind(&operation_space_geojson)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    /// Delete an entity by its identifier. Returns `true` if a row was removed.
    pub async fn delete(&self, id: &str) -> Result<bool, AndromedaError> {
        let result = sqlx::query("DELETE FROM entities WHERE id = $1").bind(id).execute(&self.pool).await?;

        Ok(result.rows_affected() > 0)
    }

    /// Execute a compiled entity query and return matching payloads with an optional total count.
    pub async fn query_compiled(&self, compiled: &CompiledEntityQuery) -> Result<QueryResult, AndromedaError> {
        let built = sql::entity::build_query(compiled)?;

        tracing::info!("Built SQL: {} with binds: {:?}", built.sql, built.bind_values);

        let rows = self.execute_dynamic_query(&built.sql, &built.bind_values).await?;

        let total_count = if let Some(count_sql) = &built.count_sql {
            let count = self.execute_count_query(count_sql, &built.bind_values).await?;
            Some(count)
        } else {
            None
        };

        Ok(QueryResult { rows, total_count })
    }

    async fn execute_dynamic_query(&self, sql: &str, binds: &[BindValue<'_>]) -> Result<Vec<Value>, AndromedaError> {
        let query = crate::bind_values!(sqlx::query(sql), binds);

        let rows = query.fetch_all(&self.pool).await?;
        let mut results = Vec::with_capacity(rows.len());
        for row in rows {
            let data: Value = row.try_get("payload")?;
            results.push(data);
        }
        Ok(results)
    }

    async fn execute_count_query(&self, sql: &str, binds: &[BindValue<'_>]) -> Result<i64, AndromedaError> {
        let query = crate::bind_values!(sqlx::query_scalar(sql), binds);

        let count: i64 = query.fetch_one(&self.pool).await?;
        Ok(count)
    }

    /// Fetch sys_attrs (created_at, modified_at, expires_at) for multiple entities in one query.
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

    /// Get per-type attribute summaries using SQL aggregation.
    /// Returns (type_iri, Vec<attribute_iri>) pairs.
    pub async fn get_entity_type_summaries(&self) -> Result<Vec<(String, Vec<String>)>, AndromedaError> {
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
            ORDER BY t
            "#,
        )
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

    /// Get attribute metadata aggregated at the SQL level.
    /// Returns (attr_iri, count, attribute_types, entity_types) tuples.
    pub async fn get_attribute_summary(&self) -> Result<Vec<AttributeSummaryRow>, AndromedaError> {
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
            ORDER BY attr_key
            "#,
        )
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

    /// Fetch attribute summary for a single attribute IRI (avoids scanning all attributes).
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

    // -------------------------------------------------------------------------
    // Bulk batch operations (UNNEST-based, single round-trip per batch)
    // -------------------------------------------------------------------------

    /// Insert multiple entities in a single SQL statement using `UNNEST`.
    ///
    /// Duplicate IDs (entities that already exist) are silently skipped via
    /// `ON CONFLICT (id) DO NOTHING`.  Returns the IDs of successfully inserted
    /// entities; the caller compares against the input to detect conflicts.
    pub async fn create_batch(&self, entities: &[PreparedEntityRow]) -> Result<Vec<String>, AndromedaError> {
        if entities.is_empty() {
            return Ok(Vec::new());
        }

        let DecomposedColumns {
            ids,
            types_json,
            scope_paths_json,
            payloads,
            locations,
            obs_spaces,
            op_spaces,
            expires_ats,
        } = decompose_rows(entities);

        let rows: Vec<(String,)> = sqlx::query_as(
            r#"
            INSERT INTO entities (id, types, scope_paths, payload, location, observation_space, operation_space, expires_at)
            SELECT
                d.id,
                ARRAY(SELECT jsonb_array_elements_text(d.types_json)),
                ARRAY(SELECT jsonb_array_elements_text(d.scope_paths_json)::ltree),
                d.payload,
                CASE WHEN d.loc IS NOT NULL THEN ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON(d.loc), 4326)) ELSE NULL END,
                CASE WHEN d.obs IS NOT NULL THEN ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON(d.obs), 4326)) ELSE NULL END,
                CASE WHEN d.ops IS NOT NULL THEN ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON(d.ops), 4326)) ELSE NULL END,
                d.expires_at
            FROM UNNEST($1::text[], $2::jsonb[], $3::jsonb[], $4::jsonb[], $5::text[], $6::text[], $7::text[], $8::timestamptz[])
                AS d(id, types_json, scope_paths_json, payload, loc, obs, ops, expires_at)
            ON CONFLICT (id) DO NOTHING
            RETURNING id
            "#,
        )
        .bind(&ids)
        .bind(&types_json)
        .bind(&scope_paths_json)
        .bind(&payloads)
        .bind(&locations)
        .bind(&obs_spaces)
        .bind(&op_spaces)
        .bind(&expires_ats)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    /// Delete multiple entities in a single SQL statement.
    ///
    /// Returns full entity data (payload + timestamps) for each deleted row so
    /// the caller can emit change events without additional queries.  Entities
    /// not found are simply absent from the result.
    pub async fn delete_batch(&self, ids: &[String]) -> Result<Vec<DeletedEntity>, AndromedaError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let rows = sqlx::query("DELETE FROM entities WHERE id = ANY($1) RETURNING id, payload, created_at, modified_at, expires_at")
            .bind(ids)
            .fetch_all(&self.pool)
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

    /// Update multiple existing entities in a single SQL statement using `UNNEST`.
    ///
    /// Only rows where `entities.id = d.id` are updated; non-existent IDs are
    /// silently skipped.  Returns the IDs of successfully updated entities.
    pub async fn save_batch(&self, entities: &[PreparedEntityRow]) -> Result<Vec<String>, AndromedaError> {
        if entities.is_empty() {
            return Ok(Vec::new());
        }

        let DecomposedColumns {
            ids,
            types_json,
            scope_paths_json,
            payloads,
            locations,
            obs_spaces,
            op_spaces,
            expires_ats,
        } = decompose_rows(entities);

        let rows: Vec<(String,)> = sqlx::query_as(
            r#"
            UPDATE entities SET
                payload = d.payload,
                types = ARRAY(SELECT jsonb_array_elements_text(d.types_json)),
                scope_paths = ARRAY(SELECT jsonb_array_elements_text(d.scope_paths_json)::ltree),
                location = CASE WHEN d.loc IS NOT NULL THEN ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON(d.loc), 4326)) ELSE NULL END,
                observation_space = CASE WHEN d.obs IS NOT NULL THEN ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON(d.obs), 4326)) ELSE NULL END,
                operation_space = CASE WHEN d.ops IS NOT NULL THEN ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON(d.ops), 4326)) ELSE NULL END,
                modified_at = NOW(),
                expires_at = COALESCE(d.expires_at, entities.expires_at)
            FROM UNNEST($1::text[], $2::jsonb[], $3::jsonb[], $4::jsonb[], $5::text[], $6::text[], $7::text[], $8::timestamptz[])
                AS d(id, types_json, scope_paths_json, payload, loc, obs, ops, expires_at)
            WHERE entities.id = d.id
            RETURNING entities.id
            "#,
        )
        .bind(&ids)
        .bind(&types_json)
        .bind(&scope_paths_json)
        .bind(&payloads)
        .bind(&locations)
        .bind(&obs_spaces)
        .bind(&op_spaces)
        .bind(&expires_ats)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    /// Insert or update multiple entities in a single SQL statement.
    ///
    /// Uses `ON CONFLICT (id) DO UPDATE` to upsert.  Returns `(id, was_created)`
    /// pairs where `was_created` is `true` for newly inserted rows and `false`
    /// for updated rows (determined via PostgreSQL's `xmax = 0` pattern).
    pub async fn upsert_batch(&self, entities: &[PreparedEntityRow]) -> Result<Vec<(String, bool)>, AndromedaError> {
        if entities.is_empty() {
            return Ok(Vec::new());
        }

        let DecomposedColumns {
            ids,
            types_json,
            scope_paths_json,
            payloads,
            locations,
            obs_spaces,
            op_spaces,
            expires_ats,
        } = decompose_rows(entities);

        let rows = sqlx::query(
            r#"
            INSERT INTO entities (id, types, scope_paths, payload, location, observation_space, operation_space, expires_at)
            SELECT
                d.id,
                ARRAY(SELECT jsonb_array_elements_text(d.types_json)),
                ARRAY(SELECT jsonb_array_elements_text(d.scope_paths_json)::ltree),
                d.payload,
                CASE WHEN d.loc IS NOT NULL THEN ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON(d.loc), 4326)) ELSE NULL END,
                CASE WHEN d.obs IS NOT NULL THEN ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON(d.obs), 4326)) ELSE NULL END,
                CASE WHEN d.ops IS NOT NULL THEN ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON(d.ops), 4326)) ELSE NULL END,
                d.expires_at
            FROM UNNEST($1::text[], $2::jsonb[], $3::jsonb[], $4::jsonb[], $5::text[], $6::text[], $7::text[], $8::timestamptz[])
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
        .bind(&ids)
        .bind(&types_json)
        .bind(&scope_paths_json)
        .bind(&payloads)
        .bind(&locations)
        .bind(&obs_spaces)
        .bind(&op_spaces)
        .bind(&expires_ats)
        .fetch_all(&self.pool)
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

/// Parallel column vectors extracted from [`PreparedEntityRow`] slices for
/// sqlx `UNNEST` binding.
#[derive(Debug)]
struct DecomposedColumns {
    ids: Vec<String>,
    types_json: Vec<Value>,
    scope_paths_json: Vec<Value>,
    payloads: Vec<Value>,
    locations: Vec<Option<String>>,
    obs_spaces: Vec<Option<String>>,
    op_spaces: Vec<Option<String>>,
    expires_ats: Vec<Option<DateTime<Utc>>>,
}

/// Decompose a slice of [`PreparedEntityRow`] into parallel column vectors for
/// sqlx `UNNEST` binding.
fn decompose_rows(entities: &[PreparedEntityRow]) -> DecomposedColumns {
    let len = entities.len();
    let mut ids = Vec::with_capacity(len);
    let mut types_json = Vec::with_capacity(len);
    let mut scope_paths_json = Vec::with_capacity(len);
    let mut payloads = Vec::with_capacity(len);
    let mut locations: Vec<Option<String>> = Vec::with_capacity(len);
    let mut obs_spaces: Vec<Option<String>> = Vec::with_capacity(len);
    let mut op_spaces: Vec<Option<String>> = Vec::with_capacity(len);
    let mut expires_ats: Vec<Option<DateTime<Utc>>> = Vec::with_capacity(len);

    for e in entities {
        ids.push(e.id.clone()); // clone: owned String needed for sqlx bind vector
        types_json.push(e.types_json.clone()); // clone: owned Value needed for sqlx bind vector
        scope_paths_json.push(e.scope_paths_json.clone()); // clone: owned Value needed for sqlx bind vector
        payloads.push(e.payload.clone()); // clone: owned Value needed for sqlx bind vector
        locations.push(e.location.clone()); // clone: owned Option<String> needed for sqlx bind vector
        obs_spaces.push(e.observation_space.clone()); // clone: owned Option<String> needed for sqlx bind vector
        op_spaces.push(e.operation_space.clone()); // clone: owned Option<String> needed for sqlx bind vector
        expires_ats.push(e.expires_at);
    }

    DecomposedColumns {
        ids,
        types_json,
        scope_paths_json,
        payloads,
        locations,
        obs_spaces,
        op_spaces,
        expires_ats,
    }
}
