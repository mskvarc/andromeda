//! Row-to-domain mapping for context source registrations.

use crate::core::error::AndromedaError;
use andromeda_domain::{
    registration::{ContextSourceRegistration, ContextSourceRegistrationType, CsrOperation, CsrStatus, ForwardingMode},
    scope::ScopeBuf,
    tenant::TenantName,
    uri::Uri,
};
use chrono::{DateTime, Utc};
use iso8601_duration::Duration as Iso8601Duration;
use mediatype::MediaTypeBuf;
use serde_json::Value;
use sqlx::Row;
use url::Url;

/// Convert a `context_source_registrations` row into the domain type.
pub fn row_to_csr(row: &sqlx::postgres::PgRow) -> Result<ContextSourceRegistration, AndromedaError> {
    let id_raw: String = row.get("id");
    let id: Uri = id_raw
        .parse()
        .map_err(|e| AndromedaError::internal(format!("Invalid CSR id '{id_raw}': {e}")))?;
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
    let endpoint_raw: String = row.get("endpoint");
    let endpoint = Url::parse(&endpoint_raw).map_err(|e| AndromedaError::internal(format!("Invalid CSR endpoint '{endpoint_raw}': {e}")))?;
    let accept: Option<MediaTypeBuf> = row
        .get::<Option<String>, _>("accept")
        .map(|raw| raw.parse())
        .transpose()
        .map_err(|e| AndromedaError::internal(format!("Invalid CSR accept media type: {e}")))?;
    // `mode` and `status` are projected as `::text`; the PostgreSQL enum labels
    // and the domain `EnumString` share one definition, so a label the broker
    // does not know becomes a typed parse error here rather than an opaque
    // sqlx decode failure deep in the driver.
    let mode_label: String = row.get("mode");
    let mode = mode_label
        .parse::<ForwardingMode>()
        .map_err(|e| AndromedaError::internal(format!("Invalid CSR mode '{mode_label}': {e}")))?;
    let operations: Option<Vec<CsrOperation>> = row
        .get::<Option<Vec<String>>, _>("operations")
        .or_else(|| Some(vec!["federationOps".to_string()]))
        .map(|v| {
            v.into_iter()
                .map(|s| {
                    s.parse::<CsrOperation>()
                        .map_err(|_| AndromedaError::internal(format!("Invalid CSR operation '{s}'")))
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?;
    let scope: Option<Vec<ScopeBuf>> = row
        .get::<Option<Vec<String>>, _>("scope")
        .map(|v| {
            v.into_iter()
                .map(|s| ScopeBuf::new(s).map_err(|e| AndromedaError::internal(format!("Invalid scope value in DB: {e}"))))
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?;
    let refresh_rate: Option<Iso8601Duration> = row
        .get::<Option<String>, _>("refresh_rate")
        .map(|raw| raw.parse::<Iso8601Duration>().map_err(|e| format!("{e:?}")))
        .transpose()
        .map_err(|e| AndromedaError::internal(format!("Invalid CSR refreshRate: {e}")))?;
    let management: Option<Value> = row.get("management");
    let context_source_info: Option<Value> = row.get("context_source_info");
    let jsonld_context: Option<Url> = row
        .get::<Option<String>, _>("jsonld_context")
        .as_deref()
        .map(Url::parse)
        .transpose()
        .map_err(|e| AndromedaError::internal(format!("Invalid CSR jsonldContext: {e}")))?;
    let context_source_alias: Option<String> = match row.try_get("context_source_alias") {
        Ok(v) => v,
        Err(sqlx::Error::ColumnNotFound(_)) | Err(sqlx::Error::ColumnDecode { .. }) => None,
        Err(e) => return Err(AndromedaError::internal(format!("Failed to read context_source_alias column: {e}"))),
    };
    let status_label: String = row.get("status");
    let status = status_label
        .parse::<CsrStatus>()
        .map_err(|e| AndromedaError::internal(format!("Invalid CSR status '{status_label}': {e}")))?;
    let times_sent: i32 = row.get("times_sent");
    let times_failed: i32 = row.get("times_failed");
    let last_success: Option<DateTime<Utc>> = row.get("last_success");
    let last_failure: Option<DateTime<Utc>> = row.get("last_failure");
    let created_at: DateTime<Utc> = row.get("created_at");
    let modified_at: DateTime<Utc> = row.get("modified_at");
    let additional_properties: Option<Value> = row.get("additional_properties");

    let information = serde_json::from_value(information_json).map_err(|e| AndromedaError::internal(format!("Failed to parse CSR information: {e}")))?;

    Ok(ContextSourceRegistration {
        id,
        type_: ContextSourceRegistrationType,
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
