//! Row-to-domain mapping for context source registrations.

use crate::{
    csource_registration::{
        mode_code::mode_of,
        operation_code::operation_of,
        status_code::status_of,
        stored_registration_error::{StoredIntervalError, StoredRegistrationError},
    },
    error::ContextStoreError,
    json_text::JsonText,
};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{
    context_source_pseudonym::ContextSourcePseudonym,
    dereferenceable_uri::DereferenceableUri,
    duration::{Iso8601Duration, Iso8601DurationError},
    identifier::Identifier,
    non_empty_text::NonEmptyText,
    positive_milliseconds::PositiveMilliseconds,
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
        info::RegistrationInfo,
        lifecycle::RegistrationLifecycle,
        operation::CsrOperation,
        status::CsrStatus,
    },
    registration_management::{LocalScope, RegistrationManagementInfo},
    time_interval::TimeInterval,
};
use andromeda_ngsi_ld_query::dataset::DatasetSelector;
use andromeda_postgresql_codes::{csr_mode::CsrModeCode, csr_operation::CsrOperationCode, csr_status::CsrStatusCode};
use chrono::{DateTime, Utc};
use mediatype::MediaTypeBuf;
use serde::de::DeserializeOwned;
use sqlx::{Row, postgres::PgRow};

/// Read a nullable `jsonb` column into the typed value it was written from,
/// from the column's text, with `unreadable` naming the column when its text
/// does not decode.
fn read_json<T: DeserializeOwned>(
    row: &PgRow,
    column: &str,
    unreadable: fn(serde_json::Error) -> StoredRegistrationError,
) -> Result<Option<T>, ContextStoreError> {
    let text: Option<JsonText<'_>> = row.try_get(column)?;
    Ok(text.map(|text| serde_json::from_slice(text.as_bytes()).map_err(unreadable)).transpose()?)
}

/// Convert a `context_source_registrations` row and the `information` its
/// `registration_information` rows assemble to into the domain type.
///
/// # Errors
///
/// Returns [`ContextStoreError::StoredRegistration`] when a stored column does
/// not parse back into its domain type: the registration URI, the
/// `registrationName` or `description` text, the tenant name, the coverage
/// members, the endpoint, media type, forwarding mode code, operation codes,
/// refresh rate, `contextSourceAlias` and remaining access members, or the
/// health status code, and [`ContextStoreError::Database`] when a column the
/// projection carries is missing or does not decode into its stored type.
pub fn row_to_csr(row: &PgRow, information: Vec<RegistrationInfo>) -> Result<ContextSourceRegistration, ContextStoreError> {
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
    let additional_properties = read_json(row, "additional_properties", StoredRegistrationError::AdditionalProperties)?.unwrap_or_default();

    Ok(ContextSourceRegistration::builder()
        .id(id)
        .registration_name(registration_name)
        .description(description)
        .tenant(tenant)
        .coverage(row_to_coverage(row, information)?)
        .access(row_to_access(row)?)
        .health(row_to_health(row)?)
        .lifecycle(row_to_lifecycle(row)?)
        .additional_properties(additional_properties)
        .build())
}

/// Rebuild the coverage members: what context the registration claims to serve.
fn row_to_coverage(row: &PgRow, information: Vec<RegistrationInfo>) -> Result<RegistrationCoverage, ContextStoreError> {
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
    let observation_interval = interval_of(row.try_get("observation_interval_start")?, row.try_get("observation_interval_end")?)
        .map_err(StoredRegistrationError::ObservationInterval)?;
    let management_interval =
        interval_of(row.try_get("management_interval_start")?, row.try_get("management_interval_end")?).map_err(StoredRegistrationError::ManagementInterval)?;
    let location = read_json::<Geometry>(row, "location", StoredRegistrationError::Location)?;
    let observation_space = read_json::<Geometry>(row, "observation_space", StoredRegistrationError::ObservationSpace)?;
    let operation_space = read_json::<Geometry>(row, "operation_space", StoredRegistrationError::OperationSpace)?;

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
    let refresh_rate = duration_of(row.try_get("refresh_rate")?, StoredRegistrationError::RefreshRate)?;
    let management = management_of(StoredManagement {
        declared: row.try_get("is_management_declared")?,
        cache_duration: row.try_get("management_cache_duration")?,
        cooldown: row.try_get("management_cooldown")?,
        local_only: row.try_get("management_local_only")?,
        timeout: row.try_get("management_timeout")?,
    })?;
    let context_source_info = context_source_info_of(row.try_get("context_source_info_keys")?, row.try_get("context_source_info_values")?)?;
    let context_source_alias = row_to_alias(row)?;

    Ok(ContextSourceAccess::builder()
        .endpoint(endpoint)
        .accept(accept)
        .mode(Some(mode))
        .operations(operations)
        .refresh_rate(refresh_rate)
        .management(management)
        .context_source_info(context_source_info)
        .context_source_alias(context_source_alias)
        .build())
}

/// The clause 5.2.11 interval two stored columns hold; `None` when neither
/// holds an instant.
fn interval_of(start_at: Option<DateTime<Utc>>, end_at: Option<DateTime<Utc>>) -> Result<Option<TimeInterval>, StoredIntervalError> {
    match (start_at, end_at) {
        (None, None) => Ok(None),
        (None, Some(_)) => Err(StoredIntervalError::EndWithoutStart),
        (Some(start_at), Some(end_at)) if end_at <= start_at => Err(StoredIntervalError::EndNotAfterStart),
        (Some(start_at), end_at) => Ok(Some(TimeInterval { start_at, end_at })),
    }
}

/// The ISO 8601 duration a nullable text column holds, with `unreadable`
/// naming the column when its text does not parse.
fn duration_of(
    stored: Option<String>,
    unreadable: fn(Iso8601DurationError) -> StoredRegistrationError,
) -> Result<Option<Iso8601Duration>, StoredRegistrationError> {
    stored.map(|text| text.parse::<Iso8601Duration>()).transpose().map_err(unreadable)
}

/// The `management` columns of one registration row.
#[derive(Debug)]
struct StoredManagement {
    /// `is_management_declared`.
    declared: bool,
    /// `management_cache_duration`.
    cache_duration: Option<String>,
    /// `management_cooldown`.
    cooldown: Option<i64>,
    /// `management_local_only`.
    local_only: Option<bool>,
    /// `management_timeout`.
    timeout: Option<i64>,
}

/// The `management` member (Table 5.2.34-1) the columns hold; `None` when the
/// registration declares none.
fn management_of(stored: StoredManagement) -> Result<Option<RegistrationManagementInfo>, StoredRegistrationError> {
    if !stored.declared {
        let holds_member = stored.cache_duration.is_some() || stored.cooldown.is_some() || stored.local_only.is_some() || stored.timeout.is_some();
        return if holds_member {
            Err(StoredRegistrationError::UndeclaredManagement)
        } else {
            Ok(None)
        };
    }
    let cache_duration = duration_of(stored.cache_duration, StoredRegistrationError::ManagementCacheDuration)?;
    let cooldown = stored
        .cooldown
        .map(PositiveMilliseconds::try_from)
        .transpose()
        .map_err(StoredRegistrationError::ManagementCooldown)?;
    let timeout = stored
        .timeout
        .map(PositiveMilliseconds::try_from)
        .transpose()
        .map_err(StoredRegistrationError::ManagementTimeout)?;
    Ok(Some(
        RegistrationManagementInfo::builder()
            .cache_duration(cache_duration)
            .cooldown(cooldown)
            .local_only(stored.local_only.map(LocalScope::from))
            .timeout(timeout)
            .build(),
    ))
}

/// The `contextSourceInfo` pairs the two parallel columns hold, in written
/// order; `None` when both are `NULL`.
fn context_source_info_of(keys: Option<Vec<String>>, values: Option<Vec<String>>) -> Result<Option<Vec<KeyValuePair>>, StoredRegistrationError> {
    match (keys, values) {
        (None, None) => Ok(None),
        (Some(keys), Some(values)) if keys.len() == values.len() => Ok(Some(
            keys.into_iter()
                .zip(values)
                .map(|(key, value)| KeyValuePair {
                    key: key.into(),
                    value: value.into(),
                })
                .collect(),
        )),
        (keys, values) => Err(StoredRegistrationError::ContextSourceInfoPairs {
            keys: keys.as_ref().map(Vec::len),
            values: values.as_ref().map(Vec::len),
        }),
    }
}

/// The pseudonym a distributed operation attributes forwarded context to;
/// `None` when the registration declares none.
///
/// Every registration read selects the one shared projection, which carries the
/// column, so a column that is missing or does not decode is a failed read, not
/// a registration without an alias.
fn row_to_alias(row: &PgRow) -> Result<Option<ContextSourcePseudonym>, ContextStoreError> {
    Ok(row
        .try_get::<Option<String>, _>("context_source_alias")?
        .map(ContextSourcePseudonym::try_from)
        .transpose()
        .map_err(StoredRegistrationError::ContextSourceAlias)?)
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
/// attempt has happened at all, because the health update writes them with
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

#[cfg(test)]
mod tests {
    use crate::csource_registration::{
        mapper::{StoredManagement, context_source_info_of, duration_of, interval_of, management_of},
        registration_columns::{RegistrationFields, registration_columns},
        stored_registration_error::{StoredIntervalError, StoredRegistrationError},
    };
    use andromeda_ngsi_ld_lexicon::{duration::Iso8601Duration, time::date_time::NgsiLdDateTime};
    use andromeda_ngsi_ld_model::{
        key_value_pair::KeyValuePair,
        registration_management::{LocalScope, RegistrationManagementInfo},
        time_interval::TimeInterval,
    };
    use andromeda_postgresql_schema::csr::Csr;
    use andromeda_postgresql_seaquery::external_arguments::ExternalArguments;
    use andromeda_registration::{
        nullable_member::NullableMember,
        resolved_update::apply_fragment,
        store::{create_params::CreateCsrParams, update_fragment::CsrUpdateFragment},
    };
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use sea_query::{SimpleExpr, Value};
    use serde_json::json;

    fn instant(raw: &str) -> DateTime<Utc> {
        raw.parse::<NgsiLdDateTime>().expect("a valid instant").get()
    }

    fn management(cache_duration: Option<&str>, cooldown: Option<i64>, local_only: Option<bool>, timeout: Option<i64>) -> StoredManagement {
        StoredManagement {
            declared: true,
            cache_duration: cache_duration.map(str::to_owned),
            cooldown,
            local_only,
            timeout,
        }
    }

    #[test]
    fn the_intervals_read_back_as_written() {
        let open: TimeInterval = serde_json::from_value(json!({"startAt": "2026-01-01T00:00:00Z"})).expect("a valid interval");
        let closed: TimeInterval =
            serde_json::from_value(json!({"startAt": "2026-01-01T00:00:00Z", "endAt": "2026-02-01T00:00:00Z"})).expect("a valid interval");
        assert_eq!(interval_of(Some(instant("2026-01-01T00:00:00Z")), None), Ok(Some(open)));
        assert_eq!(
            interval_of(Some(instant("2026-01-01T00:00:00Z")), Some(instant("2026-02-01T00:00:00Z"))),
            Ok(Some(closed))
        );
        assert_eq!(interval_of(None, None), Ok(None));
    }

    #[test]
    fn an_end_without_a_start_is_refused() {
        assert_eq!(
            interval_of(None, Some(instant("2026-02-01T00:00:00Z"))),
            Err(StoredIntervalError::EndWithoutStart)
        );
        assert_eq!(
            interval_of(Some(instant("2026-02-01T00:00:00Z")), Some(instant("2026-02-01T00:00:00Z"))),
            Err(StoredIntervalError::EndNotAfterStart)
        );
    }

    #[test]
    fn the_management_members_read_back_as_written() {
        let read = management_of(management(Some("PT10S"), Some(500), Some(true), Some(3000)))
            .expect("the members read back")
            .expect("management is declared");
        let written: RegistrationManagementInfo =
            serde_json::from_value(json!({"cacheDuration": "PT10S", "cooldown": 500, "localOnly": true, "timeout": 3000})).expect("valid management");
        assert_eq!(read, written);
        assert_eq!(read.local_only, Some(LocalScope::LocalOnly));
        assert_eq!(
            management_of(management(None, None, None, None)).expect("an empty management reads back"),
            Some(RegistrationManagementInfo::default())
        );
        let undeclared = StoredManagement {
            declared: false,
            ..management(None, None, None, None)
        };
        assert_eq!(management_of(undeclared).expect("no management reads back"), None);
    }

    fn zero() -> Iso8601Duration {
        "PT0S".parse().expect("a zero duration")
    }

    fn zero_cache_duration() -> RegistrationManagementInfo {
        serde_json::from_value(json!({"cacheDuration": "PT0S"})).expect("valid management")
    }

    /// `fields` binds `PT0S` to the `refresh_rate` and
    /// `management_cache_duration` columns, and each reads back as the zero
    /// duration it was written from.
    fn assert_zero_durations_round_trip(fields: &RegistrationFields<'_>) {
        let mut external = ExternalArguments::default();
        let columns = registration_columns(fields, None, &mut external).expect("the columns build");
        let bound = |wanted: fn(&Csr) -> bool| {
            columns.iter().find(|(column, _)| wanted(column)).and_then(|(_, value)| {
                if let SimpleExpr::Value(Value::String(text)) = value {
                    text.clone() // clone: the columns are borrowed and the test owns what it reads back
                } else {
                    None
                }
            })
        };
        let refresh_rate = bound(|column| matches!(column, Csr::RefreshRate));
        let cache_duration = bound(|column| matches!(column, Csr::ManagementCacheDuration));
        assert_eq!(refresh_rate.as_deref(), Some("PT0S"));
        assert_eq!(cache_duration.as_deref(), Some("PT0S"));
        let read_cache_duration = management_of(management(cache_duration.as_deref(), None, None, None))
            .expect("the management reads back")
            .and_then(|management| management.cache_duration);
        assert_eq!(read_cache_duration, Some(zero()));
        assert_eq!(
            duration_of(refresh_rate, StoredRegistrationError::RefreshRate).expect("the refreshRate reads back"),
            Some(zero())
        );
    }

    /// Tables 5.2.9-1 and 5.2.34-1 bound neither duration below, so a zero one
    /// is stored as text the store reads back.
    #[test]
    fn a_registration_created_with_zero_durations_stores_and_reads_back_pt0s() {
        let params = CreateCsrParams::builder()
            .information(Vec::new())
            .endpoint("https://example.org/csource".parse().expect("a dereferenceable URI"))
            .operations(Vec::new())
            .refresh_rate(Some(zero()))
            .management(Some(zero_cache_duration()))
            .build();
        assert_zero_durations_round_trip(&RegistrationFields::from(&params));
    }

    #[test]
    fn a_registration_updated_to_zero_durations_stores_and_reads_back_pt0s() {
        let existing = serde_json::from_value(json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:zero-durations",
            "type": "ContextSourceRegistration",
            "information": [{"propertyNames": ["speed"]}],
            "endpoint": "https://example.org/csource",
            "refreshRate": "PT1H",
            "management": {"cacheDuration": "PT10S"},
            "createdAt": "2026-01-01T00:00:00Z",
            "modifiedAt": "2026-01-01T00:00:00Z"
        }))
        .expect("a stored registration");
        let fragment = CsrUpdateFragment {
            refresh_rate: NullableMember::Set(zero()),
            management: NullableMember::Set(zero_cache_duration()),
            ..CsrUpdateFragment::default()
        };
        let resolved = apply_fragment(existing, &fragment);
        assert_zero_durations_round_trip(&RegistrationFields::from(&resolved));
    }

    /// A bare `P` has no component, so it is no ISO 8601 duration.
    #[test]
    fn a_stored_bare_designator_is_refused() {
        assert!(matches!(
            duration_of(Some("P".to_owned()), StoredRegistrationError::RefreshRate),
            Err(StoredRegistrationError::RefreshRate(_))
        ));
    }

    #[test]
    fn a_stored_duration_that_does_not_parse_is_refused() {
        assert!(matches!(
            management_of(management(Some("ten seconds"), None, None, None)),
            Err(StoredRegistrationError::ManagementCacheDuration(_))
        ));
    }

    #[test]
    fn a_non_positive_cooldown_is_refused() {
        assert!(matches!(
            management_of(management(None, Some(0), None, None)),
            Err(StoredRegistrationError::ManagementCooldown(_))
        ));
        assert!(matches!(
            management_of(management(None, None, None, Some(-1))),
            Err(StoredRegistrationError::ManagementTimeout(_))
        ));
    }

    #[test]
    fn a_member_of_an_undeclared_management_is_refused() {
        let undeclared = StoredManagement {
            declared: false,
            ..management(None, Some(500), None, None)
        };
        assert!(matches!(management_of(undeclared), Err(StoredRegistrationError::UndeclaredManagement)));
    }

    #[test]
    fn the_context_source_info_reads_back_in_written_order() {
        let pairs =
            context_source_info_of(Some(vec!["z".to_owned(), "a".to_owned()]), Some(vec!["1".to_owned(), "2".to_owned()])).expect("the pairs read back");
        let written: Vec<KeyValuePair> = serde_json::from_value(json!([{"key": "z", "value": "1"}, {"key": "a", "value": "2"}])).expect("valid pairs");
        assert_eq!(pairs, Some(written));
        assert_eq!(context_source_info_of(None, None).expect("absent"), None);
        assert_eq!(context_source_info_of(Some(Vec::new()), Some(Vec::new())).expect("empty"), Some(Vec::new()));
    }

    #[test]
    fn context_source_info_arrays_of_different_lengths_are_refused() {
        assert!(matches!(
            context_source_info_of(Some(vec!["a".to_owned()]), Some(Vec::new())),
            Err(StoredRegistrationError::ContextSourceInfoPairs {
                keys: Some(1),
                values: Some(0)
            })
        ));
        assert!(matches!(
            context_source_info_of(Some(vec!["a".to_owned()]), None),
            Err(StoredRegistrationError::ContextSourceInfoPairs { keys: Some(1), values: None })
        ));
    }
}
