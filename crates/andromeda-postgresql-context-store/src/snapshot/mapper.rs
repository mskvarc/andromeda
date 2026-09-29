//! Row-to-domain conversions for snapshot records.

use crate::{error::ContextStoreError, snapshot::status_code::status_of};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{
    key_value_pair::KeyValuePair,
    query::body::Query,
    snapshot::{ExecutionResultDetails, Snapshot, SnapshotPriority},
};
use andromeda_ngsi_ld_number::json_value::deserialize_exact;
use andromeda_postgresql_codes::snapshot_status::SnapshotStatusCode;
use iso8601_duration::Duration as Iso8601Duration;
use serde::de::DeserializeOwned;
use serde_json::Value;
use sqlx::{Row, postgres::PgRow};
use url::Url;

/// Deserialize an optional JSON column into a typed `Option<T>`.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the column cannot be read, and
/// [`ContextStoreError::Internal`] when the stored JSON does not deserialize
/// into `T`.
pub fn deserialize_json_column<T: DeserializeOwned>(row: &PgRow, column: &str) -> Result<Option<T>, ContextStoreError> {
    let val: Option<Value> = row.try_get(column)?;
    val.map(|v| deserialize_exact(&v).map_err(|e| ContextStoreError::Internal(format!("Failed to deserialize {column}: {e}"))))
        .transpose()
}

/// Convert a `snapshots` row into the domain [`Snapshot`].
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when a column cannot be read, and
/// [`ContextStoreError::Internal`] when a stored value no longer parses back
/// into its domain type: the Snapshot URI, the status code, the lifetime
/// duration, the endpoint URL, or one of the JSON-encoded members.
pub fn row_to_snapshot(row: &PgRow) -> Result<Snapshot, ContextStoreError> {
    let id_str: String = row.try_get("id")?;
    let id = id_str
        .parse::<Identifier>()
        .map_err(|e| ContextStoreError::Internal(format!("Invalid snapshot id '{id_str}': {e}")))?;

    // An unassigned status code is a typed decode failure naming the registry
    // rather than an opaque sqlx one.
    let status_code: i16 = row.try_get("status")?;
    let status = status_of(SnapshotStatusCode::try_from(status_code).map_err(|e| ContextStoreError::Internal(format!("Invalid snapshot status: {e}")))?);

    let snapshot_lifetime = row
        .try_get::<Option<String>, _>("snapshot_lifetime")?
        .as_deref()
        .map(str::parse::<Iso8601Duration>)
        .transpose()
        .map_err(|e| ContextStoreError::Internal(format!("Invalid snapshot lifetime: {e:?}")))?;
    // The `snapshot_priority` column is `SMALLINT`; narrow it back to the byte
    // the domain type validates. A value outside `u8` cannot be a valid
    // priority, so it maps to `0`, which the range check then rejects.
    let snapshot_priority_raw: i16 = row.try_get("snapshot_priority")?;
    let snapshot_priority = SnapshotPriority::try_from(u8::try_from(snapshot_priority_raw).unwrap_or(0))
        .map_err(|e| ContextStoreError::Internal(format!("Invalid snapshot priority '{snapshot_priority_raw}': {e}")))?;
    let endpoint = row
        .try_get::<Option<String>, _>("endpoint")?
        .map(|value| Url::parse(&value))
        .transpose()
        .map_err(|e| ContextStoreError::Internal(format!("Invalid endpoint URI: {e}")))?;

    let queries_details: Option<Vec<ExecutionResultDetails>> = deserialize_json_column(row, "queries_details")?;
    let temporal_queries_details: Option<Vec<ExecutionResultDetails>> = deserialize_json_column(row, "temporal_queries_details")?;
    let snapshot_queries: Option<Vec<Query>> = deserialize_json_column(row, "snapshot_queries")?;
    let snapshot_temporal_queries: Option<Vec<Query>> = deserialize_json_column(row, "snapshot_temporal_queries")?;
    let receiver_info: Option<Vec<KeyValuePair>> = deserialize_json_column(row, "receiver_info")?;

    Ok(Snapshot::builder()
        .id(id)
        .snapshot_queries(snapshot_queries)
        .snapshot_temporal_queries(snapshot_temporal_queries)
        .snapshot_lifetime(snapshot_lifetime)
        .snapshot_priority(snapshot_priority)
        .endpoint(endpoint)
        .receiver_info(receiver_info)
        .status(status)
        .queries_details(queries_details)
        .temporal_queries_details(temporal_queries_details)
        .created_at(row.try_get("created_at")?)
        .modified_at(row.try_get("modified_at")?)
        .expires_at(row.try_get("expires_at")?)
        .last_used_at(row.try_get("last_used_at")?)
        .build())
}
