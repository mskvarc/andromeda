//! Row-to-domain mapping for context source registrations.

use crate::{
    csource_registration::{mode_code::mode_of, operation_code::operation_of, status_code::status_of, stored_registration_error::StoredRegistrationError},
    error::ContextStoreError,
};
use andromeda_ngsi_ld_lexicon::{
    context_source_pseudonym::ContextSourcePseudonym,
    dereferenceable_uri::DereferenceableUri,
    identifier::Identifier,
    non_empty_text::NonEmptyText,
    scope::path::ScopePath,
    tenant::TenantName,
};
use andromeda_ngsi_ld_model::{
    key_value_pair::KeyValuePair,
    registration::{
        ContextSourceRegistration,
        access::ContextSourceAccess,
        coverage::RegistrationCoverage,
        health::RegistrationHealth,
        lifecycle::RegistrationLifecycle,
        operation::CsrOperation,
        status::CsrStatus,
    },
    registration_management::RegistrationManagementInfo,
    time_interval::TimeInterval,
};
use andromeda_ngsi_ld_query::dataset::DatasetSelector;
use andromeda_postgresql_codes::{csr_mode::CsrModeCode, csr_operation::CsrOperationCode, csr_status::CsrStatusCode};
use chrono::{DateTime, Utc};
use iso8601_duration::Duration as Iso8601Duration;
use mediatype::MediaTypeBuf;
use serde_json::Value;
use sqlx::{Row, postgres::PgRow};

/// Convert a `context_source_registrations` row into the domain type.
///
/// # Errors
///
/// Returns [`ContextStoreError::StoredRegistration`] when a stored column does
/// not parse back into its domain type: the registration URI, the
/// `registrationName` or `description` text, the tenant name, the `information`
/// document and the rest of the coverage members, the endpoint, media type,
/// forwarding mode code, operation codes, refresh rate and remaining access
/// members, or the health status code, and [`ContextStoreError::Database`] when
/// a column the projection carries is missing or does not decode into its
/// stored type, or when the `context_source_alias` column exists but fails to
/// read for a reason other than being absent or undecodable.
pub fn row_to_csr(row: &PgRow) -> Result<ContextSourceRegistration, ContextStoreError> {
    let id_raw: String = row.try_get("id")?;
    let id: Identifier = id_raw.parse().map_err(StoredRegistrationError::Id)?;
    let registration_name = row
        .try_get::<Option<String>, _>("registration_name")?
        .map(NonEmptyText::try_from)
        .transpose()
        .map_err(StoredRegistrationError::RegistrationName)?;
    let description = row
        .try_get::<Option<String>, _>("description")?
        .map(NonEmptyText::try_from)
        .transpose()
        .map_err(StoredRegistrationError::Description)?;
    let tenant = row
        .try_get::<Option<String>, _>("tenant")?
        .map(TenantName::try_from)
        .transpose()
        .map_err(StoredRegistrationError::Tenant)?;
    let additional_properties: Option<Value> = row.try_get("additional_properties")?;

    Ok(ContextSourceRegistration::builder()
        .id(id)
        .registration_name(registration_name)
        .description(description)
        .tenant(tenant)
        .coverage(row_to_coverage(row)?)
        .access(row_to_access(row)?)
        .health(row_to_health(row)?)
        .lifecycle(row_to_lifecycle(row)?)
        .additional_properties(additional_properties)
        .build())
}

/// Rebuild the coverage members: what context the registration claims to serve.
fn row_to_coverage(row: &PgRow) -> Result<RegistrationCoverage, ContextStoreError> {
    let information_json: Value = row.try_get("information")?;
    let information = serde_json::from_value(information_json).map_err(StoredRegistrationError::Information)?;
    let dataset_id = row
        .try_get::<Option<Vec<String>>, _>("dataset_id")?
        .map(|values| values.iter().map(|raw| DatasetSelector::parse(raw)).collect::<Result<Vec<_>, _>>())
        .transpose()
        .map_err(|error| StoredRegistrationError::DatasetId(Box::new(error)))?;
    // Read from the text each Scope was written in, by the one clause 4.18
    // parser; the label paths beside it are only ever matched.
    let scope: Option<Vec<ScopePath>> = row
        .try_get::<Option<Vec<String>>, _>("verbatim_scope_paths")?
        .map(|values| {
            values
                .iter()
                .map(|stored| stored.parse::<ScopePath>().map_err(StoredRegistrationError::Scope))
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?;
    let observation_interval = row
        .try_get::<Option<Value>, _>("observation_interval")?
        .map(serde_json::from_value::<TimeInterval>)
        .transpose()
        .map_err(StoredRegistrationError::ObservationInterval)?;
    let management_interval = row
        .try_get::<Option<Value>, _>("management_interval")?
        .map(serde_json::from_value::<TimeInterval>)
        .transpose()
        .map_err(StoredRegistrationError::ManagementInterval)?;
    let location: Option<Value> = row.try_get("location")?;
    let observation_space: Option<Value> = row.try_get("observation_space")?;
    let operation_space: Option<Value> = row.try_get("operation_space")?;

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
    let endpoint_raw: String = row.try_get("endpoint")?;
    let endpoint = endpoint_raw.parse::<DereferenceableUri>().map_err(StoredRegistrationError::Endpoint)?;
    let accept: Option<MediaTypeBuf> = row
        .try_get::<Option<String>, _>("accept")?
        .map(|raw| raw.parse())
        .transpose()
        .map_err(StoredRegistrationError::Accept)?;
    // A number the registry never assigned is a typed decode failure naming
    // the registry, rather than an opaque sqlx one deep in the driver.
    let mode = mode_of(CsrModeCode::try_from(row.try_get::<i16, _>("mode")?).map_err(StoredRegistrationError::Mode)?);
    let operations: Option<Vec<CsrOperation>> = row
        .try_get::<Option<Vec<i16>>, _>("operations")?
        .map(|codes| {
            codes
                .into_iter()
                .map(|code| CsrOperationCode::try_from(code).map(operation_of).map_err(StoredRegistrationError::Operation))
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?;
    let refresh_rate: Option<Iso8601Duration> = row
        .try_get::<Option<String>, _>("refresh_rate")?
        .map(|raw| raw.parse::<Iso8601Duration>())
        .transpose()
        .map_err(|error| StoredRegistrationError::RefreshRate { position: error.position })?;
    let management = row
        .try_get::<Option<Value>, _>("management")?
        .map(serde_json::from_value::<RegistrationManagementInfo>)
        .transpose()
        .map_err(StoredRegistrationError::Management)?;
    let context_source_info = row
        .try_get::<Option<Value>, _>("context_source_info")?
        .map(serde_json::from_value::<Vec<KeyValuePair>>)
        .transpose()
        .map_err(StoredRegistrationError::ContextSourceInfo)?;
    let jsonld_context: Option<DereferenceableUri> = row
        .try_get::<Option<String>, _>("jsonld_context")?
        .as_deref()
        .map(str::parse)
        .transpose()
        .map_err(StoredRegistrationError::JsonldContext)?;
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
            .map_err(|error| StoredRegistrationError::ContextSourceAlias(error).into()),
        Err(sqlx::Error::ColumnNotFound(_) | sqlx::Error::ColumnDecode { .. }) => Ok(None),
        Err(error) => Err(error.into()),
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
/// for the absence of evidence. The delivery state still carries the status
/// the registration started with; the timestamps are what say whether an
/// attempt has happened at all, because `health_update_sql` writes them with
/// the status in one statement.
///
/// The delivery state is `LEFT JOIN`ed, so a registration whose state row is
/// missing reads as one that has observed nothing.
fn row_to_health(row: &PgRow) -> Result<RegistrationHealth, ContextStoreError> {
    let last_success: Option<DateTime<Utc>> = row.try_get("last_success")?;
    let last_failure: Option<DateTime<Utc>> = row.try_get("last_failure")?;
    let attempted = last_success.is_some() || last_failure.is_some();

    let status: Option<CsrStatus> = row
        .try_get::<Option<i16>, _>("status")?
        .filter(|_| attempted)
        .map(|code| CsrStatusCode::try_from(code).map(status_of))
        .transpose()
        .map_err(StoredRegistrationError::Status)?;

    Ok(RegistrationHealth::builder()
        .status(status)
        .times_sent(row.try_get::<Option<i32>, _>("times_sent")?.and_then(observed_counter))
        .times_failed(row.try_get::<Option<i32>, _>("times_failed")?.and_then(observed_counter))
        .last_success(last_success)
        .last_failure(last_failure)
        .build())
}

/// The member a Table 5.2.9-2 counter renders as.
///
/// The column floors at zero because a counter has to be somewhere before it is
/// first advanced. A counter still at rest reports nothing, which is the
/// absence the member's 0..1 cardinality provides for; a negative count is not
/// reachable through the column's own `CHECK` and is read the same way, as is
/// the absent counter of a registration without a delivery-state row.
const fn observed_counter(counted: i32) -> Option<i32> {
    if counted > 0 { Some(counted) } else { None }
}

/// Rebuild the lifecycle members: the timestamps the broker maintains itself.
fn row_to_lifecycle(row: &PgRow) -> Result<RegistrationLifecycle, ContextStoreError> {
    let expires_at: Option<DateTime<Utc>> = row.try_get("expires_at")?;
    let created_at: DateTime<Utc> = row.try_get("created_at")?;
    let modified_at: DateTime<Utc> = row.try_get("modified_at")?;

    Ok(RegistrationLifecycle::builder()
        .expires_at(expires_at)
        .created_at(created_at)
        .modified_at(modified_at)
        .build())
}
