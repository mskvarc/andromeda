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

/// Result of an entity query containing matched rows and an optional total count.
pub struct QueryResult {
    /// The matched entity payloads.
    pub rows: Vec<Value>,
    /// Total number of matching entities, present when a count was requested.
    pub total_count: Option<i64>,
}

/// Aggregated metadata for a single attribute across all entities.
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
        sqlx::query(
            r#"
            INSERT INTO entities (id, types, scopes, payload, location, observation_space, operation_space, expires_at)
            VALUES ($1, $2, $3, $4,
                    CASE WHEN $5 IS NOT NULL THEN ST_SetSRID(ST_GeomFromGeoJSON($5), 4326) ELSE NULL END,
                    CASE WHEN $6 IS NOT NULL THEN ST_SetSRID(ST_GeomFromGeoJSON($6), 4326) ELSE NULL END,
                    CASE WHEN $7 IS NOT NULL THEN ST_SetSRID(ST_GeomFromGeoJSON($7), 4326) ELSE NULL END,
                    $8)
            "#,
        )
        .bind(id)
        .bind(type_)
        .bind(scopes)
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
                FROM jsonb_each(payload->0) AS kv(key, val),
                     LATERAL jsonb_array_elements(val) AS inst(elem)
                WHERE kv.key NOT LIKE '@%'
                  AND (inst.elem #>>'{https://uri.etsi.org/ngsi-ld/expiresAt,0,@value}')::timestamptz <= NOW()
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
        let scopes = expanded::extract_scopes(entity);
        let location_geojson = expanded::extract_location_geojson(entity);
        let observation_space_geojson = expanded::extract_observation_space_geojson(entity);
        let operation_space_geojson = expanded::extract_operation_space_geojson(entity);
        let expires_at = expanded::extract_expires_at(entity);
        let result = sqlx::query(
            r#"
            UPDATE entities
            SET payload = $2, types = $3, scopes = $4,
                location = CASE WHEN $5 IS NOT NULL THEN ST_SetSRID(ST_GeomFromGeoJSON($5), 4326) ELSE NULL END,
                observation_space = CASE WHEN $6 IS NOT NULL THEN ST_SetSRID(ST_GeomFromGeoJSON($6), 4326) ELSE NULL END,
                operation_space = CASE WHEN $7 IS NOT NULL THEN ST_SetSRID(ST_GeomFromGeoJSON($7), 4326) ELSE NULL END,
                modified_at = NOW(),
                expires_at = COALESCE($8, expires_at)
            WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(entity)
        .bind(&types)
        .bind(&scopes)
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
        let built = sql::build_query(compiled)?;

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
        let mut query = sqlx::query(sql);
        for bind in binds {
            query = match bind {
                BindValue::String(s) => query.bind(s.as_ref()),
                BindValue::StringVec(v) => query.bind(v.as_ref()),
                BindValue::I64(i) => query.bind(i),
            };
        }

        let rows = query.fetch_all(&self.pool).await?;
        let mut results = Vec::with_capacity(rows.len());
        for row in rows {
            let data: Value = row.try_get("payload")?;
            results.push(data);
        }
        Ok(results)
    }

    async fn execute_count_query(&self, sql: &str, binds: &[BindValue<'_>]) -> Result<i64, AndromedaError> {
        let mut query = sqlx::query_scalar(sql);
        for bind in binds {
            query = match bind {
                BindValue::String(s) => query.bind(s.as_ref()),
                BindValue::StringVec(v) => query.bind(v.as_ref()),
                BindValue::I64(i) => query.bind(i),
            };
        }

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

        let mut result = HashMap::with_capacity(ids.len());
        for row in rows {
            let id: String = row.get("id");
            result.insert(
                id,
                SysAttrsSnapshot {
                    created_at: row.get("created_at"),
                    modified_at: row.get("modified_at"),
                    expires_at: row.get("expires_at"),
                },
            );
        }
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

        let mut results = Vec::new();
        for row in rows {
            let type_iri: String = row.get("type_iri");
            let attr_keys: Vec<String> = row.get("attr_keys");
            results.push((type_iri, attr_keys));
        }
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

        let mut results = Vec::new();
        for row in rows {
            let attribute_types: Option<Vec<String>> = row.get("attribute_types");
            results.push(AttributeSummaryRow {
                attr_iri: row.get("attr_key"),
                entity_count: row.get("entity_count"),
                entity_types: row.get("entity_types"),
                attribute_types: attribute_types.unwrap_or_default(),
            });
        }
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
        let built = sql::build_ids_query(compiled)?;

        tracing::info!("Built IDs SQL: {} with binds: {:?}", built.sql, built.bind_values);

        let mut query = sqlx::query(&built.sql);
        for bind in &built.bind_values {
            query = match bind {
                BindValue::String(s) => query.bind(s.as_ref()),
                BindValue::StringVec(v) => query.bind(v.as_ref()),
                BindValue::I64(i) => query.bind(i),
            };
        }

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
}
