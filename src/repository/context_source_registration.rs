//! Repository for NGSI-LD context source registrations (CSRs).
//!
//! Provides CRUD, query, and discovery operations for CSR records stored in
//! PostgreSQL, including geometry handling and additional property merging.

use crate::{
    domain::{
        registration::{ContextSourceRegistration, CsrOperation, CsrStatus, ForwardingMode},
        scope::ScopeBuf,
        uri::NGSI_LD_URN_NID,
    },
    error::AndromedaError,
    query::{compiled::CompiledCsrQuery, csr_sql},
    tenancy::types::TenantName,
};
use chrono::{DateTime, Utc};
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
    management, context_source_info, jsonld_context, context_source_alias, \
    status, times_sent, times_failed, \
    last_success, last_failure, created_at, modified_at, \
    additional_properties";

/// Parameters for creating a new context source registration.
#[derive(Debug)]
pub struct CreateCsrParams<'a> {
    /// Human-readable name of the registration.
    pub registration_name: Option<&'a str>,
    /// Free-text description of the registration.
    pub description: Option<&'a str>,
    /// Registration information entries describing provided entities/attributes.
    pub information: Option<&'a Value>,
    /// Tenant identifier for multi-tenancy.
    pub tenant: Option<&'a str>,
    /// Expected interval between observations.
    pub observation_interval: Option<&'a Value>,
    /// Expected interval between management interactions.
    pub management_interval: Option<&'a Value>,
    /// GeoJSON location of the context source.
    pub location: Option<&'a Value>,
    /// GeoJSON observation space of the context source.
    pub observation_space: Option<&'a Value>,
    /// GeoJSON operation space of the context source.
    pub operation_space: Option<&'a Value>,
    /// Expiration timestamp for the registration.
    pub expires_at: Option<DateTime<Utc>>,
    /// Base endpoint URL of the context source.
    pub endpoint: Option<&'a str>,
    /// Preferred MIME type for responses from the context source.
    pub accept: Option<&'a str>,
    /// Forwarding mode (e.g. "auxiliary", "inclusive", "redirect", "exclusive").
    pub mode: &'a str,
    /// NGSI-LD operations supported by the context source.
    pub operations: Option<Vec<String>>,
    /// Scope values for the registration.
    pub scope: Option<Vec<String>>,
    /// ISO 8601 duration between context source refreshes.
    pub refresh_rate: Option<&'a str>,
    /// Management information for the registration.
    pub management: Option<&'a Value>,
    /// Additional context source metadata entries.
    pub context_source_info: Option<&'a Value>,
    /// JSON-LD context associated with this registration.
    pub jsonld_context: Option<&'a str>,
    /// Unique pseudonym for Via-based loop detection (clause 5.2.9).
    pub context_source_alias: Option<&'a str>,
    /// Non-standard properties preserved from the original request.
    pub additional_properties: Option<Value>,
}

impl CreateCsrParams<'_> {
    /// Keys recognized as standard NGSI-LD CSR properties (used to separate additional properties).
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
        "contextSourceAlias",
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
#[derive(Debug)]
pub struct CsrUpdateFragment {
    /// Updated registration name.
    pub registration_name: Option<Option<String>>,
    /// Updated description.
    pub description: Option<Option<String>>,
    /// Updated registration information entries.
    pub information: Option<Value>,
    /// Updated tenant identifier.
    pub tenant: Option<Option<TenantName>>,
    /// Updated observation interval.
    pub observation_interval: Option<Option<Value>>,
    /// Updated management interval.
    pub management_interval: Option<Option<Value>>,
    /// Updated GeoJSON location (as serialized string).
    pub location: Option<Option<String>>,
    /// Updated GeoJSON observation space (as serialized string).
    pub observation_space: Option<Option<String>>,
    /// Updated GeoJSON operation space (as serialized string).
    pub operation_space: Option<Option<String>>,
    /// Updated expiration timestamp.
    pub expires_at: Option<Option<DateTime<Utc>>>,
    /// Updated endpoint URL.
    pub endpoint: Option<String>,
    /// Updated accepted MIME type.
    pub accept: Option<Option<String>>,
    /// Updated forwarding mode.
    pub mode: Option<String>,
    /// Updated supported operations.
    pub operations: Option<Option<Vec<String>>>,
    /// Updated scope values.
    pub scope: Option<Option<Vec<String>>>,
    /// Updated refresh rate duration.
    pub refresh_rate: Option<Option<String>>,
    /// Updated management information.
    pub management: Option<Option<Value>>,
    /// Updated context source metadata entries.
    pub context_source_info: Option<Option<Value>>,
    /// Updated JSON-LD context reference.
    pub jsonld_context: Option<Option<String>>,
    /// Updated context source alias pseudonym.
    pub context_source_alias: Option<Option<String>>,
    /// Updated non-standard properties.
    pub additional_properties: Option<Value>,
}

/// PostgreSQL-backed repository for NGSI-LD context source registrations.
#[derive(Debug)]
pub struct ContextSourceRegistrationRepository {
    pool: PgPool,
}

impl ContextSourceRegistrationRepository {
    /// Creates a new repository backed by the given connection pool.
    pub fn new(pool: &PgPool) -> ContextSourceRegistrationRepository {
        ContextSourceRegistrationRepository { pool: pool.clone() } // clone: Arc ref-count bump (PgPool is Arc-based)
    }

    /// Generates a new unique URN identifier for a context source registration.
    pub fn generate_id() -> Result<String, AndromedaError> {
        let urn = UrnBuilder::new(NGSI_LD_URN_NID, &format!("ContextSourceRegistration:{}", Uuid::new_v4())).build()?;
        Ok(urn.to_string())
    }

    /// Checks whether a context source registration with the given ID exists.
    pub async fn exists(&self, id: &str) -> Result<bool, AndromedaError> {
        let row: (bool,) = sqlx::query_as("SELECT EXISTS(SELECT 1 FROM context_source_registrations WHERE id = $1)")
            .bind(id)
            .fetch_one(&self.pool)
            .await?;
        Ok(row.0)
    }

    /// Persists a new context source registration with the given ID and parameters.
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
                context_source_alias, additional_properties,
                created_at, modified_at
            ) VALUES (
                $1, 'ContextSourceRegistration', $2, $3, $4,
                $5, $6, $7,
                CASE WHEN $8::TEXT IS NOT NULL THEN ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON($8), 4326)) ELSE NULL END,
                CASE WHEN $9::TEXT IS NOT NULL THEN ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON($9), 4326)) ELSE NULL END,
                CASE WHEN $10::TEXT IS NOT NULL THEN ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON($10), 4326)) ELSE NULL END,
                $11, $12, $13, $14,
                $15, $16, $17,
                $18, $19, $20,
                $21, $22,
                NOW(), NOW()
            )
            "#,
        )
        .bind(id)
        .bind(params.registration_name)
        .bind(params.description)
        .bind(params.information.cloned()) // clone: sqlx bind requires owned Value
        .bind(params.tenant)
        .bind(params.observation_interval.cloned()) // clone: sqlx bind requires owned Value
        .bind(params.management_interval.cloned()) // clone: sqlx bind requires owned Value
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
        .bind(params.management.cloned()) // clone: sqlx bind requires owned Value
        .bind(params.context_source_info.cloned()) // clone: sqlx bind requires owned Value
        .bind(params.jsonld_context)
        .bind(params.context_source_alias)
        .bind(&params.additional_properties)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Retrieves a context source registration by ID.
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

    /// Lists context source registrations with pagination. Returns entries and total count.
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

    /// Executes a pre-compiled CSR query and returns matching registrations with total count.
    pub async fn query_compiled(&self, compiled: &CompiledCsrQuery) -> Result<(Vec<ContextSourceRegistration>, i64), AndromedaError> {
        let built = csr_sql::build_csr_query(compiled)?;

        // Execute count query
        let total = if let Some(ref count_sql) = built.count_sql {
            let count_query = crate::bind_values!(sqlx::query_as::<_, (i64,)>(count_sql), &built.bind_values);
            let (total,) = count_query.fetch_one(&self.pool).await?;
            total
        } else {
            0
        };

        // Execute main query
        let main_query = crate::bind_values!(sqlx::query(&built.sql), &built.bind_values);
        let rows = main_query.fetch_all(&self.pool).await?;

        let csrs = rows.iter().map(row_to_csr).collect::<Result<Vec<_>, _>>()?;

        Ok((csrs, total))
    }

    /// Applies a partial update to an existing CSR. Returns `false` if the ID was not found.
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
            Some(v) => v.clone(), // clone: conditional ownership -- fragment or existing
            None => serde_json::to_value(&existing.information)?,
        };
        let tenant: Option<TenantName> = match &fragment.tenant {
            Some(v) => v.clone(), // clone: conditional ownership -- fragment or existing
            None => existing.tenant,
        };
        let observation_interval: Option<Value> = match &fragment.observation_interval {
            Some(v) => v.clone(), // clone: conditional ownership -- fragment or existing
            None => existing.observation_interval,
        };
        let management_interval: Option<Value> = match &fragment.management_interval {
            Some(v) => v.clone(), // clone: conditional ownership -- fragment or existing
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
            Some(v) => v.clone(), // clone: conditional ownership -- fragment or existing
            None => existing.accept,
        };
        let mode_str = match &fragment.mode {
            Some(v) => v.clone(), // clone: conditional ownership -- fragment or existing
            None => existing.mode.unwrap_or_default().to_string(),
        };
        let operations: Option<Vec<String>> = match &fragment.operations {
            Some(v) => v.clone(), // clone: conditional ownership -- fragment or existing
            None => existing.operations.map(|ops| ops.into_iter().map(|op| op.to_string()).collect()),
        };
        let scope: Option<Vec<String>> = match &fragment.scope {
            Some(v) => v.clone(), // clone: conditional ownership -- fragment or existing
            None => existing.scope.map(|v| v.into_iter().map(|s| s.to_string()).collect()),
        };
        let refresh_rate: Option<String> = match &fragment.refresh_rate {
            Some(v) => v.clone(), // clone: conditional ownership -- fragment or existing
            None => existing.refresh_rate,
        };
        let management: Option<Value> = match &fragment.management {
            Some(v) => v.clone(), // clone: conditional ownership -- fragment or existing
            None => existing.management,
        };
        let context_source_info: Option<Value> = match &fragment.context_source_info {
            Some(v) => v.clone(), // clone: conditional ownership -- fragment or existing
            None => existing.context_source_info,
        };
        let jsonld_context: Option<String> = match &fragment.jsonld_context {
            Some(v) => v.clone(), // clone: conditional ownership -- fragment or existing
            None => existing.jsonld_context,
        };
        let context_source_alias: Option<String> = match &fragment.context_source_alias {
            Some(v) => v.clone(), // clone: conditional ownership -- fragment or existing
            None => existing.context_source_alias,
        };

        // Merge additional properties: new fragment's extras override existing
        let additional_properties = match (existing.additional_properties, &fragment.additional_properties) {
            (Some(existing_val), Some(new_val)) => {
                let mut merged = existing_val;
                if let (Some(m), Some(n)) = (merged.as_object_mut(), new_val.as_object()) {
                    for (k, v) in n {
                        m.insert(k.clone(), v.clone()); // clone: owned key/value for map insertion
                    }
                }
                Some(merged)
            }
            (None, Some(new_val)) => Some(new_val.clone()), // clone: owned value from borrowed fragment
            (Some(existing_val), None) => Some(existing_val),
            (None, None) => None,
        };

        // Geometry: use new value if provided, else keep existing (as GeoJSON text).
        let location_geojson = match &fragment.location {
            Some(v) => v.clone(), // clone: conditional ownership -- fragment or existing
            None => existing.location.map(|v| v.to_string()),
        };
        let observation_space_geojson = match &fragment.observation_space {
            Some(v) => v.clone(), // clone: conditional ownership -- fragment or existing
            None => existing.observation_space.map(|v| v.to_string()),
        };
        let operation_space_geojson = match &fragment.operation_space {
            Some(v) => v.clone(), // clone: conditional ownership -- fragment or existing
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
                location = CASE WHEN $8::TEXT IS NOT NULL THEN ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON($8), 4326)) ELSE NULL END,
                observation_space = CASE WHEN $9::TEXT IS NOT NULL THEN ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON($9), 4326)) ELSE NULL END,
                operation_space = CASE WHEN $10::TEXT IS NOT NULL THEN ST_MakeValid(ST_SetSRID(ST_GeomFromGeoJSON($10), 4326)) ELSE NULL END,
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
                context_source_alias = $21,
                additional_properties = $22,
                modified_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(registration_name)
        .bind(description)
        .bind(serde_json::to_value(&information)?)
        .bind(tenant.as_ref().map(TenantName::as_str))
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
        .bind(context_source_alias.as_deref())
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

    /// Returns all non-expired CSRs without entity type/id filtering.
    ///
    /// Used for discovery forwarding (types, attributes) where the broker needs to
    /// query all registered context sources regardless of entity coverage.
    pub async fn find_all_active(&self) -> Result<Vec<ContextSourceRegistration>, AndromedaError> {
        let sql = format!(
            "SELECT {} FROM context_source_registrations WHERE (expires_at IS NULL OR expires_at > NOW()) ORDER BY created_at ASC",
            COLUMNS
        );
        let rows = sqlx::query(&sql).fetch_all(&self.pool).await?;
        rows.iter().map(row_to_csr).collect()
    }

    /// Deletes a context source registration by ID. Returns `true` if a row was removed.
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
    let tenant_str: Option<String> = row.get("tenant");
    let tenant = tenant_str
        .map(TenantName::try_from)
        .transpose()
        .map_err(|e| AndromedaError::internal(e.to_string()))?;
    let observation_interval: Option<Value> = row.get("observation_interval");
    let management_interval: Option<Value> = row.get("management_interval");
    let location: Option<Value> = row.get("location");
    let observation_space: Option<Value> = row.get("observation_space");
    let operation_space: Option<Value> = row.get("operation_space");
    let expires_at: Option<DateTime<Utc>> = row.get("expires_at");
    let endpoint: String = row.get("endpoint");
    let accept: Option<String> = row.get("accept");
    let mode_str: String = row.get("mode");
    let mode: ForwardingMode = mode_str
        .parse()
        .map_err(|e: strum::ParseError| AndromedaError::internal(format!("Invalid forwarding mode '{}': {}", mode_str, e)))?;
    let operations: Option<Vec<CsrOperation>> = row
        .get::<Option<Vec<String>>, _>("operations")
        .or_else(|| Some(vec!["federationOps".to_string()]))
        .map(|v| {
            v.into_iter()
                .map(|s| {
                    s.parse::<CsrOperation>()
                        .map_err(|_| AndromedaError::internal(format!("Invalid CSR operation '{}'", s)))
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?;
    let scope: Option<Vec<ScopeBuf>> = row
        .get::<Option<Vec<String>>, _>("scope")
        .map(|v| {
            v.into_iter()
                .map(|s| ScopeBuf::new(s).map_err(|e| AndromedaError::internal(format!("Invalid scope value in DB: {}", e))))
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?;
    let refresh_rate: Option<String> = row.get("refresh_rate");
    let management: Option<Value> = row.get("management");
    let context_source_info: Option<Value> = row.get("context_source_info");
    let jsonld_context: Option<String> = row.get("jsonld_context");
    let context_source_alias: Option<String> = match row.try_get("context_source_alias") {
        Ok(v) => v,
        Err(sqlx::Error::ColumnNotFound(_)) | Err(sqlx::Error::ColumnDecode { .. }) => None,
        Err(e) => return Err(AndromedaError::internal(format!("Failed to read context_source_alias column: {}", e))),
    };
    let status_str: String = row.get("status");
    let status: CsrStatus = status_str.parse().map_err(|e: strum::ParseError| AndromedaError::internal(e.to_string()))?;
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
        context_source_alias,
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
    if v.get("type").and_then(|t| t.as_str()) == Some("GeoProperty")
        && let Some(inner) = v.get("value")
    {
        return inner.to_string();
    }
    v.to_string()
}

#[cfg(test)]
mod tests {
    use super::unwrap_geo_property_to_string;
    use serde_json::json;

    #[test]
    fn geo_property_wrapper_returns_inner_value() {
        let input = json!({
            "type": "GeoProperty",
            "value": {"type": "Point", "coordinates": [1.0, 2.0]}
        });
        let result = unwrap_geo_property_to_string(&input);
        let expected: serde_json::Value = serde_json::from_str(&result).expect("valid JSON");
        assert_eq!(expected, json!({"type": "Point", "coordinates": [1.0, 2.0]}));
    }

    #[test]
    fn raw_geojson_returns_as_is() {
        let input = json!({"type": "Point", "coordinates": [1.0, 2.0]});
        let result = unwrap_geo_property_to_string(&input);
        let expected: serde_json::Value = serde_json::from_str(&result).expect("valid JSON");
        assert_eq!(expected, json!({"type": "Point", "coordinates": [1.0, 2.0]}));
    }

    #[test]
    fn non_geo_property_type_returns_full_value() {
        let input = json!({
            "type": "Property",
            "value": {"type": "Point", "coordinates": [1.0, 2.0]}
        });
        let result = unwrap_geo_property_to_string(&input);
        let expected: serde_json::Value = serde_json::from_str(&result).expect("valid JSON");
        assert_eq!(expected, input);
    }
}
