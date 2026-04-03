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

pub struct QueryResult {
    pub rows: Vec<Value>,
    pub total_count: Option<i64>,
}

pub struct AttributeSummaryRow {
    pub attr_iri: String,
    pub entity_count: i64,
    pub entity_types: Vec<String>,
    pub attribute_types: Vec<String>,
}

pub struct EntityRepository {
    pool: PgPool,
}

impl EntityRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn create(&self, id: &str, type_: &[&str], scopes: &[String], entity: &Value, expires_at: Option<DateTime<Utc>>) -> Result<Urn, AndromedaError> {
        tracing::info!("Storing entity {} with types: {:?}", id, type_);
        let location_geojson = expanded::extract_location_geojson(entity);
        sqlx::query(
            "INSERT INTO entities (id, types, scopes, payload, location, expires_at) VALUES ($1, $2, $3, $4, CASE WHEN $5 IS NOT NULL THEN ST_SetSRID(ST_GeomFromGeoJSON($5), 4326) ELSE NULL END, $6)",
        )
            .bind(id)
            .bind(type_)
            .bind(scopes)
            .bind(entity)
            .bind(&location_geojson)
            .bind(expires_at)
            .execute(&self.pool)
            .await?;

        Ok(id.parse::<Urn>()?)
    }

    pub async fn get(&self, id: &str) -> Result<Option<Value>, AndromedaError> {
        let result: Option<Value> = sqlx::query_scalar("SELECT payload FROM entities WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;

        Ok(result)
    }

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

    /// Save (overwrite) the expanded payload for an existing entity.
    /// Automatically re-extracts types, scopes, and location from the payload.
    /// Preserves existing expires_at unless the payload contains a new expiresAt value.
    pub async fn save(&self, id: &str, entity: &Value) -> Result<bool, AndromedaError> {
        let types = expanded::extract_types(entity);
        let scopes = expanded::extract_scopes(entity);
        let location_geojson = expanded::extract_location_geojson(entity);
        let expires_at = extract_expires_at(entity);
        let result = sqlx::query(
            r#"
            UPDATE entities
            SET payload = $2, types = $3, scopes = $4,
                location = CASE WHEN $5 IS NOT NULL THEN ST_SetSRID(ST_GeomFromGeoJSON($5), 4326) ELSE NULL END,
                modified_at = NOW(),
                expires_at = COALESCE($6, expires_at)
            WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(entity)
        .bind(&types)
        .bind(&scopes)
        .bind(&location_geojson)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn delete(&self, id: &str) -> Result<bool, AndromedaError> {
        let result = sqlx::query("DELETE FROM entities WHERE id = $1").bind(id).execute(&self.pool).await?;

        Ok(result.rows_affected() > 0)
    }

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

    pub async fn exists(&self, id: &str) -> Result<bool, AndromedaError> {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM entities WHERE id = $1)")
            .bind(id)
            .fetch_one(&self.pool)
            .await?;

        Ok(exists)
    }

    pub async fn types(&self) -> Result<Vec<String>, AndromedaError> {
        Ok(sqlx::query_scalar("SELECT DISTINCT unnest(types) FROM entities").fetch_all(&self.pool).await?)
    }

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

    pub async fn count_by_type(&self, type_: &str) -> Result<i64, AndromedaError> {
        Ok(sqlx::query_scalar("SELECT COUNT(*) FROM entities WHERE $1 = ANY(types)")
            .bind(type_)
            .fetch_one(&self.pool)
            .await?)
    }
}

/// Extract the expiresAt value from an expanded JSON-LD entity payload.
/// The expanded IRI for expiresAt is `https://uri.etsi.org/ngsi-ld/expiresAt`.
pub fn extract_expires_at(entity: &Value) -> Option<DateTime<Utc>> {
    let obj = match entity {
        Value::Array(arr) => arr.first().and_then(|v| v.as_object()),
        Value::Object(obj) => Some(obj),
        _ => return None,
    }?;

    let expires_at_val = obj.get("https://uri.etsi.org/ngsi-ld/expiresAt")?;
    let dt_str = expires_at_val
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|v| v.get("@value"))
        .and_then(|v| v.as_str())?;

    chrono::DateTime::parse_from_rfc3339(dt_str).ok().map(|dt| dt.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Datelike;
    use serde_json::json;

    #[test]
    fn extract_expires_at_from_expanded_entity() {
        let entity = json!([{
            "@id": "urn:ngsi-ld:Entity:001",
            "@type": ["https://example.com/Building"],
            "https://uri.etsi.org/ngsi-ld/expiresAt": [{
                "@type": "https://uri.etsi.org/ngsi-ld/DateTime",
                "@value": "2030-12-31T23:59:59.000Z"
            }]
        }]);
        let result = extract_expires_at(&entity);
        assert!(result.is_some());
        assert_eq!(result.unwrap().year(), 2030);
    }

    #[test]
    fn extract_expires_at_missing() {
        let entity = json!([{
            "@id": "urn:ngsi-ld:Entity:001",
            "@type": ["https://example.com/Building"]
        }]);
        assert!(extract_expires_at(&entity).is_none());
    }

    #[test]
    fn extract_expires_at_invalid_date() {
        let entity = json!([{
            "@id": "urn:ngsi-ld:Entity:001",
            "https://uri.etsi.org/ngsi-ld/expiresAt": [{
                "@type": "https://uri.etsi.org/ngsi-ld/DateTime",
                "@value": "not-a-date"
            }]
        }]);
        assert!(extract_expires_at(&entity).is_none());
    }
}
