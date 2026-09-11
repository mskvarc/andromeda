//! Row-to-domain mapping for context source registrations.

use crate::error::ContextStoreError;
use andromeda_ngsi_ld_lexicon::{
    context_source_pseudonym::ContextSourcePseudonym,
    dereferenceable_uri::DereferenceableUri,
    non_empty_text::NonEmptyText,
    scope::grammar::ScopeBuf,
    tenant::TenantName,
    uri::Uri,
};
use andromeda_ngsi_ld_model::{
    key_value_pair::KeyValuePair,
    registration::{
        ContextSourceRegistration,
        access::ContextSourceAccess,
        coverage::RegistrationCoverage,
        forwarding_mode::ForwardingMode,
        health::RegistrationHealth,
        lifecycle::RegistrationLifecycle,
        operation::CsrOperation,
        status::CsrStatus,
    },
    registration_management::RegistrationManagementInfo,
    time_interval::TimeInterval,
};
use andromeda_ngsi_ld_query::dataset::DatasetSelector;
use chrono::{DateTime, Utc};
use iso8601_duration::Duration as Iso8601Duration;
use mediatype::MediaTypeBuf;
use serde_json::Value;
use sqlx::{Row, postgres::PgRow};

/// Convert a `context_source_registrations` row into the domain type.
///
/// # Errors
///
/// Returns [`ContextStoreError::Internal`] when a stored column does not parse
/// back into its domain type: the registration URI, the `registrationName` or
/// `description` text, the tenant name, the `information` document and the rest
/// of the coverage members, the endpoint, media type, forwarding mode,
/// operations, refresh rate and remaining access members, or the health status
/// label. It is also returned when the `context_source_alias` column exists but
/// fails to read for a reason other than being absent or undecodable.
pub fn row_to_csr(row: &PgRow) -> Result<ContextSourceRegistration, ContextStoreError> {
    let id_raw: String = row.get("id");
    let id: Uri = id_raw
        .parse()
        .map_err(|e| ContextStoreError::Internal(format!("Invalid CSR id '{id_raw}': {e}")))?;
    let registration_name = row
        .get::<Option<String>, _>("registration_name")
        .map(NonEmptyText::try_from)
        .transpose()
        .map_err(|e| ContextStoreError::Internal(format!("Invalid CSR registrationName: {e}")))?;
    let description = row
        .get::<Option<String>, _>("description")
        .map(NonEmptyText::try_from)
        .transpose()
        .map_err(|e| ContextStoreError::Internal(format!("Invalid CSR description: {e}")))?;
    let tenant = row
        .get::<Option<String>, _>("tenant")
        .map(TenantName::try_from)
        .transpose()
        .map_err(|e| ContextStoreError::Internal(e.to_string()))?;
    let additional_properties: Option<Value> = row.get("additional_properties");

    Ok(ContextSourceRegistration::builder()
        .id(id)
        .registration_name(registration_name)
        .description(description)
        .tenant(tenant)
        .coverage(row_to_coverage(row)?)
        .access(row_to_access(row)?)
        .health(row_to_health(row)?)
        .lifecycle(row_to_lifecycle(row))
        .additional_properties(additional_properties)
        .build())
}

/// Rebuild the coverage members: what context the registration claims to serve.
fn row_to_coverage(row: &PgRow) -> Result<RegistrationCoverage, ContextStoreError> {
    let information_json: Value = row.get("information");
    let information = serde_json::from_value(information_json).map_err(|e| ContextStoreError::Internal(format!("Failed to parse CSR information: {e}")))?;
    let dataset_id = row
        .get::<Option<Vec<String>>, _>("dataset_id")
        .map(|values| values.iter().map(|raw| DatasetSelector::parse(raw)).collect::<Result<Vec<_>, _>>())
        .transpose()
        .map_err(|e| ContextStoreError::Internal(format!("Invalid CSR datasetId: {e}")))?;
    let scope: Option<Vec<ScopeBuf>> = row
        .get::<Option<Vec<String>>, _>("scope")
        .map(|values| {
            values
                .into_iter()
                .map(|raw| ScopeBuf::new(raw).map_err(|e| ContextStoreError::Internal(format!("Invalid scope value in DB: {e}"))))
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?;
    let observation_interval = row
        .get::<Option<Value>, _>("observation_interval")
        .map(serde_json::from_value::<TimeInterval>)
        .transpose()
        .map_err(|e| ContextStoreError::Internal(format!("Invalid CSR observationInterval: {e}")))?;
    let management_interval = row
        .get::<Option<Value>, _>("management_interval")
        .map(serde_json::from_value::<TimeInterval>)
        .transpose()
        .map_err(|e| ContextStoreError::Internal(format!("Invalid CSR managementInterval: {e}")))?;
    let location: Option<Value> = row.get("location");
    let observation_space: Option<Value> = row.get("observation_space");
    let operation_space: Option<Value> = row.get("operation_space");

    Ok(RegistrationCoverage::builder()
        .information(information)
        .dataset_id(dataset_id)
        .scope(scope)
        .location(location)
        .observation_space(observation_space)
        .operation_space(operation_space)
        .observation_interval(observation_interval)
        .management_interval(management_interval)
        .build())
}

/// Rebuild the access members: how the broker reaches the context source.
fn row_to_access(row: &PgRow) -> Result<ContextSourceAccess, ContextStoreError> {
    let endpoint_raw: String = row.get("endpoint");
    let endpoint = endpoint_raw
        .parse::<DereferenceableUri>()
        .map_err(|e| ContextStoreError::Internal(format!("Invalid CSR endpoint '{endpoint_raw}': {e}")))?;
    let accept: Option<MediaTypeBuf> = row
        .get::<Option<String>, _>("accept")
        .map(|raw| raw.parse())
        .transpose()
        .map_err(|e| ContextStoreError::Internal(format!("Invalid CSR accept media type: {e}")))?;
    // `mode` is projected as `::text`; the PostgreSQL enum labels and the domain
    // `EnumString` share one definition, so a label the broker does not know
    // becomes a typed parse error here rather than an opaque sqlx decode failure
    // deep in the driver.
    let mode_label: String = row.get("mode");
    let mode = mode_label
        .parse::<ForwardingMode>()
        .map_err(|e| ContextStoreError::Internal(format!("Invalid CSR mode '{mode_label}': {e}")))?;
    let operations: Option<Vec<CsrOperation>> = row
        .get::<Option<Vec<String>>, _>("operations")
        .map(|labels| {
            labels
                .into_iter()
                .map(|label| {
                    label
                        .parse::<CsrOperation>()
                        .map_err(|_| ContextStoreError::Internal(format!("Invalid CSR operation '{label}'")))
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?;
    let refresh_rate: Option<Iso8601Duration> = row
        .get::<Option<String>, _>("refresh_rate")
        .map(|raw| raw.parse::<Iso8601Duration>().map_err(|e| format!("{e:?}")))
        .transpose()
        .map_err(|e| ContextStoreError::Internal(format!("Invalid CSR refreshRate: {e}")))?;
    let management = row
        .get::<Option<Value>, _>("management")
        .map(serde_json::from_value::<RegistrationManagementInfo>)
        .transpose()
        .map_err(|e| ContextStoreError::Internal(format!("Invalid CSR management: {e}")))?;
    let context_source_info = row
        .get::<Option<Value>, _>("context_source_info")
        .map(serde_json::from_value::<Vec<KeyValuePair>>)
        .transpose()
        .map_err(|error| ContextStoreError::Internal(format!("Invalid CSR contextSourceInfo: {error}")))?;
    let jsonld_context: Option<DereferenceableUri> = row
        .get::<Option<String>, _>("jsonld_context")
        .as_deref()
        .map(str::parse)
        .transpose()
        .map_err(|e| ContextStoreError::Internal(format!("Invalid CSR jsonldContext: {e}")))?;
    let context_source_alias = row_to_alias(row)?;

    Ok(ContextSourceAccess::builder()
        .endpoint(endpoint)
        .accept(accept)
        .mode(Some(mode))
        .operations(operations)
        .refresh_rate(refresh_rate)
        .management(management)
        .context_source_info(context_source_info)
        .jsonld_context(jsonld_context)
        .context_source_alias(context_source_alias)
        .build())
}

/// The pseudonym a distributed operation attributes forwarded context to.
///
/// The column is absent from projections that predate it and undecodable where
/// the projection widened it, and neither case is a broken registration, so both
/// read as "no alias declared".
fn row_to_alias(row: &PgRow) -> Result<Option<ContextSourcePseudonym>, ContextStoreError> {
    match row.try_get::<Option<String>, _>("context_source_alias") {
        Ok(value) => value
            .map(ContextSourcePseudonym::try_from)
            .transpose()
            .map_err(|error| ContextStoreError::Internal(format!("Invalid CSR contextSourceAlias: {error}"))),
        Err(sqlx::Error::ColumnNotFound(_) | sqlx::Error::ColumnDecode { .. }) => Ok(None),
        Err(e) => Err(ContextStoreError::Internal(format!("Failed to read context_source_alias column: {e}"))),
    }
}

/// Rebuild the health members: the forwarding outcome counters and timestamps.
///
/// Every member of Table 5.2.9-2 has cardinality 0..1 and reports something the
/// broker has observed -- `status` "if the last attempt to perform a
/// distributed operation succeeded", `timesSent` the number of operations the
/// registration triggered, `lastSuccess` "created on first successful
/// operation". A registration that has triggered nothing has observed nothing,
/// so it reports none of them rather than a health and two zeroes that stand
/// for the absence of evidence. The stored row still carries the column the
/// forwarding outcome writes; the timestamps are what say whether an attempt
/// has happened at all, because `health_update_sql` writes them with the status
/// in one statement.
fn row_to_health(row: &PgRow) -> Result<RegistrationHealth, ContextStoreError> {
    let last_success: Option<DateTime<Utc>> = row.get("last_success");
    let last_failure: Option<DateTime<Utc>> = row.get("last_failure");
    let attempted = last_success.is_some() || last_failure.is_some();

    // `status` is projected as `::text` for the same reason as `mode`.
    let status_label: String = row.get("status");
    let status = attempted
        .then(|| {
            status_label
                .parse::<CsrStatus>()
                .map_err(|e| ContextStoreError::Internal(format!("Invalid CSR status '{status_label}': {e}")))
        })
        .transpose()?;

    Ok(RegistrationHealth::builder()
        .status(status)
        .times_sent(observed_counter(row.get("times_sent")))
        .times_failed(observed_counter(row.get("times_failed")))
        .last_success(last_success)
        .last_failure(last_failure)
        .build())
}

/// The member a Table 5.2.9-2 counter renders as.
///
/// The column floors at zero because a counter has to be somewhere before it is
/// first advanced. A counter still at rest reports nothing, which is the
/// absence the member's 0..1 cardinality provides for; a negative count is not
/// reachable through the column's own `CHECK` and is read the same way.
const fn observed_counter(counted: i32) -> Option<i32> {
    if counted > 0 { Some(counted) } else { None }
}

/// Rebuild the lifecycle members: the timestamps the broker maintains itself.
fn row_to_lifecycle(row: &PgRow) -> RegistrationLifecycle {
    let expires_at: Option<DateTime<Utc>> = row.get("expires_at");
    let created_at: DateTime<Utc> = row.get("created_at");
    let modified_at: DateTime<Utc> = row.get("modified_at");

    RegistrationLifecycle::builder()
        .expires_at(expires_at)
        .created_at(created_at)
        .modified_at(modified_at)
        .build()
}
