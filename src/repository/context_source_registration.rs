use crate::{
    error::AndromedaError,
    query::{compiled::CompiledCsrQuery, csr_sql, sql::BindValue},
};
use chrono::{DateTime, Utc};
use ngsi_ld_types::{
    registration::{ContextSourceRegistration, CsrOperation, CsrStatus, ForwardingMode},
    uri::NGSI_LD_URN_NID,
};
use serde_json::Value;
use sqlx::{Row, postgres::PgPool};
use urn::UrnBuilder;
use uuid::Uuid;

const COLUMNS: &str = "\
    id, type, registration_name, description, information, \
    tenant, observation_interval, management_interval, \
    ST_AsGeoJSON(location)::JSONB AS location, \
    ST_AsGeoJSON(observation_space)::JSONB AS observation_space, \
    ST_AsGeoJSON(operation_space)::JSONB AS operation_space, \
    expires_at, endpoint, accept, mode, \
    operations, scope, refresh_rate, \
    management, context_source_info, jsonld_context, \
    status, times_sent, times_failed, \
    last_success, last_failure, created_at, modified_at, \
    additional_properties";

pub struct CreateCsrParams<'a> {
    pub registration_name: Option<&'a str>,
    pub description: Option<&'a str>,
    pub information: Option<&'a Value>,
    pub tenant: Option<&'a str>,
    pub observation_interval: Option<&'a Value>,
    pub management_interval: Option<&'a Value>,
    pub location: Option<&'a Value>,
    pub observation_space: Option<&'a Value>,
    pub operation_space: Option<&'a Value>,
    pub expires_at: Option<DateTime<Utc>>,
    pub endpoint: Option<&'a str>,
    pub accept: Option<&'a str>,
    pub mode: &'a str,
    pub operations: Option<Vec<String>>,
    pub scope: Option<Vec<String>>,
    pub refresh_rate: Option<&'a str>,
    pub management: Option<&'a Value>,
    pub context_source_info: Option<&'a Value>,
    pub jsonld_context: Option<&'a str>,
    pub additional_properties: Option<Value>,
}

impl CreateCsrParams<'_> {
    pub const STANDARD_KEYS: &'static [&'static str] = &[
        "id",
        "type",
        "@context",
        "registrationName",
        "description",
        "information",
        "tenant",
        "observationInterval",
        "managementInterval",
        "location",
        "observationSpace",
        "operationSpace",
        "expiresAt",
        "endpoint",
        "accept",
        "mode",
        "operations",
        "scope",
        "refreshRate",
        "management",
        "contextSourceInfo",
        "jsonldContext",
        "status",
        "timesSent",
        "timesFailed",
        "lastSuccess",
        "lastFailure",
        "createdAt",
        "modifiedAt",
    ];
}

/// Typed fragment for CSR updates. `None` = field not provided (keep existing).
/// `Some(None)` = explicitly null (clear field). `Some(Some(v))` = new value.
pub struct CsrUpdateFragment {
    pub registration_name: Option<Option<String>>,
    pub description: Option<Option<String>>,
    pub information: Option<Value>,
    pub tenant: Option<Option<String>>,
    pub observation_interval: Option<Option<Value>>,
    pub management_interval: Option<Option<Value>>,
    pub location: Option<Option<String>>,
    pub observation_space: Option<Option<String>>,
    pub operation_space: Option<Option<String>>,
    pub expires_at: Option<Option<DateTime<Utc>>>,
    pub endpoint: Option<String>,
    pub accept: Option<Option<String>>,
    pub mode: Option<String>,
    pub operations: Option<Option<Vec<String>>>,
    pub scope: Option<Option<Vec<String>>>,
    pub refresh_rate: Option<Option<String>>,
    pub management: Option<Option<Value>>,
    pub context_source_info: Option<Option<Value>>,
    pub jsonld_context: Option<Option<String>>,
    pub additional_properties: Option<Value>,
}

pub struct ContextSourceRegistrationRepository {
    pool: PgPool,
}

impl ContextSourceRegistrationRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub fn generate_id() -> Result<String, AndromedaError> {
        let urn = UrnBuilder::new(NGSI_LD_URN_NID, &format!("ContextSourceRegistration:{}", Uuid::new_v4())).build()?;
        Ok(urn.to_string())
    }

    pub async fn exists(&self, id: &str) -> Result<bool, AndromedaError> {
        let row: (bool,) = sqlx::query_as("SELECT EXISTS(SELECT 1 FROM context_source_registrations WHERE id = $1)")
            .bind(id)
            .fetch_one(&self.pool)
            .await?;
        Ok(row.0)
    }

    pub async fn create(&self, id: &str, params: &CreateCsrParams<'_>) -> Result<(), AndromedaError> {
        let location_geojson = params.location.map(unwrap_geo_property_to_string);
        let observation_space_geojson = params.observation_space.map(unwrap_geo_property_to_string);
        let operation_space_geojson = params.operation_space.map(unwrap_geo_property_to_string);

        sqlx::query(
            r#"
            INSERT INTO context_source_registrations (
                id, type, registration_name, description, information,
                tenant, observation_interval, management_interval,
                location, observation_space, operation_space,
                expires_at, endpoint, accept, mode,
                operations, scope, refresh_rate,
                management, context_source_info, jsonld_context,
                additional_properties,
                created_at, modified_at
            ) VALUES (
                $1, 'ContextSourceRegistration', $2, $3, $4,
                $5, $6, $7,
                CASE WHEN $8::TEXT IS NOT NULL THEN ST_GeomFromGeoJSON($8) ELSE NULL END,
                CASE WHEN $9::TEXT IS NOT NULL THEN ST_GeomFromGeoJSON($9) ELSE NULL END,
                CASE WHEN $10::TEXT IS NOT NULL THEN ST_GeomFromGeoJSON($10) ELSE NULL END,
                $11, $12, $13, $14,
                $15, $16, $17,
                $18, $19, $20,
                $21,
                NOW(), NOW()
            )
            "#,
        )
        .bind(id)
        .bind(params.registration_name)
        .bind(params.description)
        .bind(params.information.cloned())
        .bind(params.tenant)
        .bind(params.observation_interval.cloned())
        .bind(params.management_interval.cloned())
        .bind(location_geojson.as_deref())
        .bind(observation_space_geojson.as_deref())
        .bind(operation_space_geojson.as_deref())
        .bind(params.expires_at)
        .bind(params.endpoint)
        .bind(params.accept)
        .bind(params.mode)
        .bind(params.operations.as_deref())
        .bind(params.scope.as_deref())
        .bind(params.refresh_rate)
        .bind(params.management.cloned())
        .bind(params.context_source_info.cloned())
        .bind(params.jsonld_context)
        .bind(&params.additional_properties)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get(&self, id: &str) -> Result<Option<ContextSourceRegistration>, AndromedaError> {
        let row = sqlx::query(&format!("SELECT {} FROM context_source_registrations WHERE id = $1", COLUMNS))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;

        match row {
            Some(row) => Ok(Some(row_to_csr(&row)?)),
            None => Ok(None),
        }
    }

    pub async fn list(&self, limit: Option<i64>, offset: Option<i64>) -> Result<(Vec<ContextSourceRegistration>, i64), AndromedaError> {
        let limit = limit.unwrap_or(100);
        let offset = offset.unwrap_or(0);

        let count_row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM context_source_registrations")
            .fetch_one(&self.pool)
            .await?;

        let rows = sqlx::query(&format!(
            "SELECT {} FROM context_source_registrations ORDER BY created_at ASC LIMIT $1 OFFSET $2",
            COLUMNS
        ))
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        let csrs = rows.iter().map(row_to_csr).collect::<Result<Vec<_>, _>>()?;

        Ok((csrs, count_row.0))
    }

    pub async fn query_compiled(&self, compiled: &CompiledCsrQuery) -> Result<(Vec<ContextSourceRegistration>, i64), AndromedaError> {
        let built = csr_sql::build_csr_query(compiled)?;

        // Execute count query
        let total = if let Some(ref count_sql) = built.count_sql {
            let mut count_query = sqlx::query_as::<_, (i64,)>(count_sql);
            for bind in &built.bind_values {
                count_query = match bind {
                    BindValue::String(s) => count_query.bind(s.clone()),
                    BindValue::StringVec(v) => count_query.bind(v.clone()),
                    BindValue::I64(i) => count_query.bind(*i),
                };
            }
            let (total,) = count_query.fetch_one(&self.pool).await?;
            total
        } else {
            0
        };

        // Execute main query
        let mut main_query = sqlx::query(&built.sql);
        for bind in &built.bind_values {
            main_query = match bind {
                BindValue::String(s) => main_query.bind(s.clone()),
                BindValue::StringVec(v) => main_query.bind(v.clone()),
                BindValue::I64(i) => main_query.bind(*i),
            };
        }
        let rows = main_query.fetch_all(&self.pool).await?;

        let csrs = rows.iter().map(row_to_csr).collect::<Result<Vec<_>, _>>()?;

        Ok((csrs, total))
    }

    pub async fn update(&self, id: &str, fragment: &CsrUpdateFragment) -> Result<bool, AndromedaError> {
        let mut tx = self.pool.begin().await?;

        // Lock the row to prevent concurrent updates (SELECT FOR UPDATE)
        let lock_row = sqlx::query(&format!("SELECT {} FROM context_source_registrations WHERE id = $1 FOR UPDATE", COLUMNS))
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;

        let existing = match lock_row {
            Some(row) => row_to_csr(&row)?,
            None => return Ok(false),
        };

        let registration_name = match &fragment.registration_name {
            Some(v) => v.as_deref(),
            None => existing.registration_name.as_deref(),
        };
        let description = match &fragment.description {
            Some(v) => v.as_deref(),
            None => existing.description.as_deref(),
        };
        let information: Value = match &fragment.information {
            Some(v) => v.clone(),
            None => serde_json::to_value(&existing.information).unwrap(),
        };
        let tenant: Option<String> = match &fragment.tenant {
            Some(v) => v.clone(),
            None => existing.tenant,
        };
        let observation_interval: Option<Value> = match &fragment.observation_interval {
            Some(v) => v.clone(),
            None => existing.observation_interval,
        };
        let management_interval: Option<Value> = match &fragment.management_interval {
            Some(v) => v.clone(),
            None => existing.management_interval,
        };
        let expires_at: Option<DateTime<Utc>> = match &fragment.expires_at {
            Some(v) => *v,
            None => existing.expires_at,
        };
        let endpoint = match &fragment.endpoint {
            Some(v) => v.as_str(),
            None => &existing.endpoint,
        };
        let accept: Option<String> = match &fragment.accept {
            Some(v) => v.clone(),
            None => existing.accept,
        };
        let mode_str = match &fragment.mode {
            Some(v) => v.clone(),
            None => existing.mode.unwrap_or_default().to_string(),
        };
        let operations: Option<Vec<String>> = match &fragment.operations {
            Some(v) => v.clone(),
            None => existing.operations.map(|ops| ops.into_iter().map(|op| op.to_string()).collect()),
        };
        let scope: Option<Vec<String>> = match &fragment.scope {
            Some(v) => v.clone(),
            None => existing.scope,
        };
        let refresh_rate: Option<String> = match &fragment.refresh_rate {
            Some(v) => v.clone(),
            None => existing.refresh_rate,
        };
        let management: Option<Value> = match &fragment.management {
            Some(v) => v.clone(),
            None => existing.management,
        };
        let context_source_info: Option<Value> = match &fragment.context_source_info {
            Some(v) => v.clone(),
            None => existing.context_source_info,
        };
        let jsonld_context: Option<String> = match &fragment.jsonld_context {
            Some(v) => v.clone(),
            None => existing.jsonld_context,
        };

        // Merge additional properties: new fragment's extras override existing
        let additional_properties = match (existing.additional_properties, &fragment.additional_properties) {
            (Some(existing_val), Some(new_val)) => {
                let mut merged = existing_val;
                if let (Some(m), Some(n)) = (merged.as_object_mut(), new_val.as_object()) {
                    for (k, v) in n {
                        m.insert(k.clone(), v.clone());
                    }
                }
                Some(merged)
            }
            (None, Some(new_val)) => Some(new_val.clone()),
            (Some(existing_val), None) => Some(existing_val),
            (None, None) => None,
        };

        // Geometry: use new value if provided, else keep existing (as GeoJSON text).
        let location_geojson = match &fragment.location {
            Some(v) => v.clone(),
            None => existing.location.map(|v| v.to_string()),
        };
        let observation_space_geojson = match &fragment.observation_space {
            Some(v) => v.clone(),
            None => existing.observation_space.map(|v| v.to_string()),
        };
        let operation_space_geojson = match &fragment.operation_space {
            Some(v) => v.clone(),
            None => existing.operation_space.map(|v| v.to_string()),
        };

        sqlx::query(
            r#"
            UPDATE context_source_registrations SET
                registration_name = $2,
                description = $3,
                information = $4,
                tenant = $5,
                observation_interval = $6,
                management_interval = $7,
                location = CASE WHEN $8::TEXT IS NOT NULL THEN ST_GeomFromGeoJSON($8) ELSE NULL END,
                observation_space = CASE WHEN $9::TEXT IS NOT NULL THEN ST_GeomFromGeoJSON($9) ELSE NULL END,
                operation_space = CASE WHEN $10::TEXT IS NOT NULL THEN ST_GeomFromGeoJSON($10) ELSE NULL END,
                expires_at = $11,
                endpoint = $12,
                accept = $13,
                mode = $14,
                operations = $15,
                scope = $16,
                refresh_rate = $17,
                management = $18,
                context_source_info = $19,
                jsonld_context = $20,
                additional_properties = $21,
                modified_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(registration_name)
        .bind(description)
        .bind(serde_json::to_value(&information).unwrap())
        .bind(tenant.as_deref())
        .bind(observation_interval)
        .bind(management_interval)
        .bind(location_geojson.as_deref())
        .bind(observation_space_geojson.as_deref())
        .bind(operation_space_geojson.as_deref())
        .bind(expires_at)
        .bind(endpoint)
        .bind(accept.as_deref())
        .bind(&mode_str)
        .bind(operations.as_deref())
        .bind(scope.as_deref())
        .bind(refresh_rate.as_deref())
        .bind(management)
        .bind(context_source_info)
        .bind(jsonld_context.as_deref())
        .bind(additional_properties)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(true)
    }

    /// Find CSRs that match a given entity query for distributed forwarding.
    ///
    /// Matches based on entity type and/or entity ID in the CSR's `information` array.
    /// Only returns non-expired CSRs.
    pub async fn find_matching(&self, entity_type: Option<&str>, entity_id: Option<&str>) -> Result<Vec<ContextSourceRegistration>, AndromedaError> {
        let mut conditions = vec!["(expires_at IS NULL OR expires_at > NOW())".to_string()];
        let mut entity_conditions = Vec::new();

        let (type_idx, id_idx) = match (entity_type.is_some(), entity_id.is_some()) {
            (true, true) => ("$1", "$2"),
            (true, false) => ("$1", ""),
            (false, true) => ("", "$1"),
            (false, false) => ("", ""),
        };

        if entity_type.is_some() {
            entity_conditions.push(format!(
                "EXISTS (SELECT 1 FROM jsonb_array_elements(information) AS info \
                 WHERE NOT jsonb_exists(info, 'entities') \
                    OR info->'entities' = '[]'::jsonb \
                    OR EXISTS (SELECT 1 FROM jsonb_array_elements(info->'entities') AS ent \
                       WHERE ent->>'type' = {type_idx}))"
            ));
        }

        if entity_id.is_some() {
            entity_conditions.push(format!(
                "EXISTS (SELECT 1 FROM jsonb_array_elements(information) AS info \
                 WHERE NOT jsonb_exists(info, 'entities') \
                    OR info->'entities' = '[]'::jsonb \
                    OR EXISTS (SELECT 1 FROM jsonb_array_elements(info->'entities') AS ent \
                       WHERE (NOT jsonb_exists(ent, 'id') OR ent->>'id' = {id_idx})))"
            ));
        }

        if !entity_conditions.is_empty() {
            conditions.push(format!("({})", entity_conditions.join(" AND ")));
        }

        let where_clause = format!("WHERE {}", conditions.join(" AND "));
        let sql = format!("SELECT {} FROM context_source_registrations {} ORDER BY created_at ASC", COLUMNS, where_clause);

        let mut query = sqlx::query(&sql);
        if let Some(etype) = entity_type {
            query = query.bind(etype.to_string());
        }
        if let Some(eid) = entity_id {
            query = query.bind(eid.to_string());
        }

        let rows = query.fetch_all(&self.pool).await?;
        rows.iter().map(row_to_csr).collect()
    }

    pub async fn delete(&self, id: &str) -> Result<bool, AndromedaError> {
        let result = sqlx::query("DELETE FROM context_source_registrations WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected() > 0)
    }
}

fn row_to_csr(row: &sqlx::postgres::PgRow) -> Result<ContextSourceRegistration, AndromedaError> {
    let id: String = row.get("id");
    let type_: String = row.get("type");
    let registration_name: Option<String> = row.get("registration_name");
    let description: Option<String> = row.get("description");
    let information_json: Value = row.get("information");
    let tenant: Option<String> = row.get("tenant");
    let observation_interval: Option<Value> = row.get("observation_interval");
    let management_interval: Option<Value> = row.get("management_interval");
    let location: Option<Value> = row.get("location");
    let observation_space: Option<Value> = row.get("observation_space");
    let operation_space: Option<Value> = row.get("operation_space");
    let expires_at: Option<DateTime<Utc>> = row.get("expires_at");
    let endpoint: String = row.get("endpoint");
    let accept: Option<String> = row.get("accept");
    let mode_str: String = row.get("mode");
    let mode: ForwardingMode = mode_str.parse().unwrap_or_default();
    let operations: Option<Vec<CsrOperation>> = row
        .get::<Option<Vec<String>>, _>("operations")
        .or_else(|| Some(vec!["federationOps".to_string()]))
        .map(|v| v.into_iter().map(|s| s.parse().unwrap()).collect());
    let scope: Option<Vec<String>> = row.get("scope");
    let refresh_rate: Option<String> = row.get("refresh_rate");
    let management: Option<Value> = row.get("management");
    let context_source_info: Option<Value> = row.get("context_source_info");
    let jsonld_context: Option<String> = row.get("jsonld_context");
    let status_str: String = row.get("status");
    let status: CsrStatus = status_str.parse().map_err(|e: String| AndromedaError::internal(e))?;
    let times_sent: i32 = row.get("times_sent");
    let times_failed: i32 = row.get("times_failed");
    let last_success: Option<DateTime<Utc>> = row.get("last_success");
    let last_failure: Option<DateTime<Utc>> = row.get("last_failure");
    let created_at: DateTime<Utc> = row.get("created_at");
    let modified_at: DateTime<Utc> = row.get("modified_at");
    let additional_properties: Option<Value> = row.get("additional_properties");

    let information = serde_json::from_value(information_json).map_err(|e| AndromedaError::internal(format!("Failed to parse CSR information: {}", e)))?;

    Ok(ContextSourceRegistration {
        id,
        type_,
        registration_name,
        description,
        information,
        tenant,
        observation_interval,
        management_interval,
        location,
        observation_space,
        operation_space,
        expires_at,
        endpoint,
        accept,
        mode: Some(mode),
        operations,
        scope,
        refresh_rate,
        management,
        context_source_info,
        jsonld_context,
        status,
        times_sent: Some(times_sent),
        times_failed: Some(times_failed),
        last_success,
        last_failure,
        created_at,
        modified_at,
        additional_properties,
    })
}

/// If `v` is a GeoProperty wrapper `{"type":"GeoProperty","value":{...}}`, return the inner
/// value's JSON string. Otherwise return the value as-is (assumed to be raw GeoJSON).
pub fn unwrap_geo_property_to_string(v: &Value) -> String {
    if v.get("type").and_then(|t| t.as_str()) == Some("GeoProperty") {
        if let Some(inner) = v.get("value") {
            return inner.to_string();
        }
    }
    v.to_string()
}
